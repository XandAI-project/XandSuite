import jsPDF from "jspdf";
import type { Artifact } from "@/lib/tauri";

const PAGE_WIDTH = 210;   // A4 mm
const PAGE_HEIGHT = 297;  // A4 mm
const MARGIN = 14;
const CONTENT_WIDTH = PAGE_WIDTH - MARGIN * 2;

// ── Export script injection ───────────────────────────────────────────────────

/**
 * Inject a postMessage listener into HTML before it is set as srcDoc.
 * When the parent sends { type: 'xandsuite-export-request' }, the script
 * collects all <canvas> elements as base64 PNGs and posts them back.
 */
export function injectExportScript(html: string): string {
  const script = `
<script>
(function() {
  window.addEventListener('message', function(e) {
    if (!e.data || e.data.type !== 'xandsuite-export-request') return;
    var canvases = Array.from(document.querySelectorAll('canvas'));
    var images = canvases.map(function(c) { return c.toDataURL('image/png'); });
    // Also capture the full body text for PDF text layer
    var bodyText = document.body ? document.body.innerText : '';
    window.parent.postMessage({ type: 'xandsuite-export', images: images, bodyText: bodyText }, '*');
  });
})();
</script>`;

  // Inject just before </body>, or append if no </body> present
  if (/<\/body>/i.test(html)) {
    return html.replace(/<\/body>/i, `${script}</body>`);
  }
  return html + script;
}

// ── PDF assembly ──────────────────────────────────────────────────────────────

function addWrappedText(
  doc: jsPDF,
  text: string,
  x: number,
  y: number,
  maxWidth: number,
  lineHeight: number,
  fontSize: number,
): number {
  doc.setFontSize(fontSize);
  const lines = doc.splitTextToSize(text, maxWidth);
  for (const line of lines) {
    if (y + lineHeight > PAGE_HEIGHT - MARGIN) {
      doc.addPage();
      y = MARGIN + 6;
    }
    doc.text(line, x, y);
    y += lineHeight;
  }
  return y;
}

function addHeader(doc: jsPDF, title: string, typeLabel: string): number {
  // Teal header bar
  doc.setFillColor(30, 30, 46);
  doc.rect(0, 0, PAGE_WIDTH, 20, "F");

  doc.setTextColor(205, 214, 244);
  doc.setFontSize(13);
  doc.setFont("helvetica", "bold");
  doc.text(title, MARGIN, 13);

  doc.setFontSize(8);
  doc.setFont("helvetica", "normal");
  doc.setTextColor(147, 153, 178);
  doc.text(typeLabel, PAGE_WIDTH - MARGIN, 13, { align: "right" });

  // Reset text color
  doc.setTextColor(30, 30, 46);
  return 28; // y position after header
}

/**
 * Strip common markdown syntax for plain-text PDF rendering.
 * Not a full parser — handles headings, bold, italic, code fences, links.
 */
function stripMarkdown(md: string): string {
  return md
    .replace(/^#{1,6}\s+/gm, "")
    .replace(/\*\*(.+?)\*\*/g, "$1")
    .replace(/\*(.+?)\*/g, "$1")
    .replace(/`{3}[\w]*\n?/g, "")
    .replace(/`(.+?)`/g, "$1")
    .replace(/\[(.+?)\]\(.+?\)/g, "$1")
    .replace(/^[-*+]\s+/gm, "• ")
    .replace(/^\d+\.\s+/gm, (m) => m)
    .replace(/^>\s+/gm, "  ")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

/**
 * Build and trigger download of a PDF for any artifact type.
 * @param artifact  The artifact to export.
 * @param chartImages  Base64 PNG strings collected from iframe canvas elements (html type only).
 */
export async function exportArtifactToPdf(
  artifact: Artifact,
  chartImages?: string[],
): Promise<void> {
  const doc = new jsPDF({ unit: "mm", format: "a4", orientation: "portrait" });
  const type = artifact.artifact_type;
  const typeLabel = type === "html" ? "HTML / Chart" : type.charAt(0).toUpperCase() + type.slice(1);

  let y = addHeader(doc, artifact.title, typeLabel);

  if (type === "html") {
    if (chartImages && chartImages.length > 0) {
      // Embed each canvas image, scaling to fit page width
      for (const imgData of chartImages) {
        if (!imgData || imgData === "data:,") continue;

        // Calculate image dimensions preserving aspect ratio
        const img = new Image();
        await new Promise<void>((resolve) => {
          img.onload = () => resolve();
          img.src = imgData;
        });

        const aspectRatio = img.naturalHeight / img.naturalWidth;
        const imgW = CONTENT_WIDTH;
        const imgH = Math.min(imgW * aspectRatio, PAGE_HEIGHT - y - MARGIN);

        if (y + imgH > PAGE_HEIGHT - MARGIN) {
          doc.addPage();
          y = MARGIN;
        }

        doc.addImage(imgData, "PNG", MARGIN, y, imgW, imgH);
        y += imgH + 8;
      }
    } else {
      // No canvas found — render a note
      doc.setFontSize(10);
      doc.setTextColor(100, 100, 100);
      doc.text("(No chart canvas detected in this HTML artifact)", MARGIN, y);
    }
  } else if (type === "code") {
    doc.setFont("courier", "normal");
    doc.setTextColor(30, 30, 46);
    const langNote = artifact.language ? `Language: ${artifact.language}\n\n` : "";
    y = addWrappedText(doc, langNote + artifact.content, MARGIN, y, CONTENT_WIDTH, 4.5, 8);
  } else if (type === "markdown") {
    doc.setFont("helvetica", "normal");
    doc.setTextColor(30, 30, 46);
    const plainText = stripMarkdown(artifact.content);
    y = addWrappedText(doc, plainText, MARGIN, y, CONTENT_WIDTH, 5.5, 10);
  } else {
    // text
    doc.setFont("courier", "normal");
    doc.setTextColor(30, 30, 46);
    y = addWrappedText(doc, artifact.content, MARGIN, y, CONTENT_WIDTH, 4.5, 9);
  }

  const filename = `${artifact.title.replace(/\s+/g, "_")}.pdf`;
  doc.save(filename);
}
