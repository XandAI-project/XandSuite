import { describe, it, expect } from "vitest";
import { formatBytes, truncate, cn } from "@/lib/utils";

describe("formatBytes", () => {
  it("formats 0 bytes", () => {
    expect(formatBytes(0)).toBe("0 B");
  });
  it("formats kilobytes", () => {
    expect(formatBytes(1024)).toBe("1 KB");
  });
  it("formats megabytes", () => {
    expect(formatBytes(1024 * 1024)).toBe("1 MB");
  });
  it("formats gigabytes", () => {
    expect(formatBytes(1024 * 1024 * 1024)).toBe("1 GB");
  });
  it("formats partial sizes", () => {
    expect(formatBytes(1536)).toBe("1.5 KB");
  });
});

describe("truncate", () => {
  it("does not truncate short strings", () => {
    expect(truncate("hello", 10)).toBe("hello");
  });
  it("truncates long strings", () => {
    expect(truncate("hello world", 8)).toBe("hello...");
  });
  it("handles exact length", () => {
    expect(truncate("hello", 5)).toBe("hello");
  });
});

describe("cn", () => {
  it("merges class names", () => {
    expect(cn("foo", "bar")).toBe("foo bar");
  });
  it("handles conditional classes", () => {
    expect(cn("foo", false && "bar", "baz")).toBe("foo baz");
  });
  it("deduplicates Tailwind classes", () => {
    expect(cn("p-2", "p-4")).toBe("p-4");
  });
});
