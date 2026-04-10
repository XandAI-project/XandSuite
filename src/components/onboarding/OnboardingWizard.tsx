import { useState, KeyboardEvent } from "react";
import { Zap, User, Briefcase, BookOpen, Sparkles, ChevronRight, Check } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { cn } from "@/lib/utils";
import { useSettingsStore } from "@/stores/settingsStore";

const ROLE_CHIPS = [
  "Developer",
  "Designer",
  "Writer",
  "Researcher",
  "Student",
  "Manager",
  "Analyst",
  "Other",
];

type Step = 1 | 2 | 3 | 4;

export function OnboardingWizard() {
  const { saveSettings } = useSettingsStore();

  const [step, setStep] = useState<Step>(1);
  const [name, setName] = useState("");
  const [profession, setProfession] = useState("");
  const [selectedChip, setSelectedChip] = useState<string | null>(null);
  const [about, setAbout] = useState("");
  const [saving, setSaving] = useState(false);

  // Step 2: selecting a chip sets the profession field too
  const handleChipClick = (chip: string) => {
    setSelectedChip(chip);
    if (chip !== "Other") {
      setProfession(chip);
    } else {
      setProfession("");
    }
  };

  const handleNext = () => {
    if (step < 4) setStep((s) => (s + 1) as Step);
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key !== "Enter" || e.shiftKey) return;
    if (step === 1 && !canAdvanceStep1) return;
    if (step === 2 && !canAdvanceStep2) return;
    if (step === 4) { e.preventDefault(); handleFinish(); return; }
    if (step < 4) { e.preventDefault(); handleNext(); }
  };

  const handleFinish = async () => {
    setSaving(true);
    try {
      await saveSettings({
        onboarding_completed: true,
        user_name: name.trim() || undefined,
        user_profession: profession.trim() || undefined,
        user_about: about.trim() || undefined,
      });
    } catch {
      // Settings couldn't be saved — still dismiss so the user isn't stuck
      setSaving(false);
    }
  };

  const canAdvanceStep1 = name.trim().length > 0;
  const canAdvanceStep2 = profession.trim().length > 0;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-background">
      {/* Subtle grid background */}
      <div
        className="absolute inset-0 opacity-[0.03]"
        style={{
          backgroundImage:
            "linear-gradient(to right, currentColor 1px, transparent 1px), linear-gradient(to bottom, currentColor 1px, transparent 1px)",
          backgroundSize: "40px 40px",
        }}
      />

      <div className="relative w-full max-w-lg mx-4 flex flex-col gap-8">
        {/* Step indicators */}
        <div className="flex items-center justify-center gap-2">
          {([1, 2, 3, 4] as Step[]).map((s) => (
            <div key={s} className="flex items-center gap-2">
              <div
                className={cn(
                  "w-2 h-2 rounded-full transition-all duration-300",
                  s < step
                    ? "bg-primary w-2 h-2"
                    : s === step
                    ? "bg-primary w-3 h-3 ring-4 ring-primary/20"
                    : "bg-border"
                )}
              />
              {s < 4 && <div className={cn("w-8 h-px transition-colors duration-300", s < step ? "bg-primary" : "bg-border")} />}
            </div>
          ))}
        </div>

        {/* Card */}
        <div className="bg-card border border-border rounded-2xl p-8 shadow-2xl">
          {/* ── Step 1: Welcome + Name ── */}
          {step === 1 && (
            <div className="flex flex-col gap-6 animate-in fade-in slide-in-from-right-4 duration-300">
              <div className="flex flex-col items-center gap-3 text-center">
                <div className="w-14 h-14 rounded-2xl bg-primary flex items-center justify-center shadow-lg shadow-primary/30">
                  <Zap className="w-7 h-7 text-primary-foreground" />
                </div>
                <div>
                  <h1 className="text-2xl font-bold tracking-tight">Welcome to XandSuite</h1>
                  <p className="text-muted-foreground text-sm mt-1">
                    Let's set up your workspace in a few quick steps.
                  </p>
                </div>
              </div>

              <div className="flex flex-col gap-2">
                <label className="text-sm font-medium flex items-center gap-1.5">
                  <User className="w-3.5 h-3.5 text-muted-foreground" />
                  What's your name?
                </label>
                <Input
                  autoFocus
                  placeholder="e.g. Alex"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  onKeyDown={handleKeyDown}
                  className="text-base h-11"
                />
              </div>

              <Button
                className="w-full gap-2"
                onClick={handleNext}
                disabled={!canAdvanceStep1}
              >
                Continue
                <ChevronRight className="w-4 h-4" />
              </Button>
            </div>
          )}

          {/* ── Step 2: Profession ── */}
          {step === 2 && (
            <div className="flex flex-col gap-6 animate-in fade-in slide-in-from-right-4 duration-300">
              <div className="flex flex-col gap-1">
                <div className="w-10 h-10 rounded-xl bg-violet-500/10 border border-violet-500/20 flex items-center justify-center mb-2">
                  <Briefcase className="w-5 h-5 text-violet-400" />
                </div>
                <h2 className="text-xl font-bold">What do you do?</h2>
                <p className="text-muted-foreground text-sm">
                  Helps the AI tailor its responses to your context.
                </p>
              </div>

              {/* Quick-select chips */}
              <div className="flex flex-wrap gap-2">
                {ROLE_CHIPS.map((chip) => (
                  <button
                    key={chip}
                    onClick={() => handleChipClick(chip)}
                    className={cn(
                      "px-3 py-1.5 rounded-full text-sm font-medium border transition-all",
                      selectedChip === chip
                        ? "glass-primary text-white border-blue-400/40"
                        : "bg-secondary border-border text-muted-foreground hover:text-foreground hover:border-primary/50"
                    )}
                  >
                    {chip}
                  </button>
                ))}
              </div>

              {/* Free-text (always visible; auto-populated by chip, overridable) */}
              <div className="flex flex-col gap-2">
                <label className="text-xs text-muted-foreground">
                  {selectedChip === "Other" ? "Describe your role" : "Or type your own"}
                </label>
                <Input
                  placeholder="e.g. Full-stack developer"
                  value={profession}
                  onChange={(e) => {
                    setProfession(e.target.value);
                    setSelectedChip(null);
                  }}
                  onKeyDown={handleKeyDown}
                  className="h-10"
                />
              </div>

              <div className="flex gap-2">
                <Button variant="outline" onClick={() => setStep(1)} className="flex-1">
                  Back
                </Button>
                <Button className="flex-1 gap-2" onClick={handleNext} disabled={!canAdvanceStep2}>
                  Continue
                  <ChevronRight className="w-4 h-4" />
                </Button>
              </div>
            </div>
          )}

          {/* ── Step 3: About Me ── */}
          {step === 3 && (
            <div className="flex flex-col gap-6 animate-in fade-in slide-in-from-right-4 duration-300">
              <div className="flex flex-col gap-1">
                <div className="w-10 h-10 rounded-xl bg-blue-500/10 border border-blue-500/20 flex items-center justify-center mb-2">
                  <BookOpen className="w-5 h-5 text-blue-400" />
                </div>
                <h2 className="text-xl font-bold">About you</h2>
                <p className="text-muted-foreground text-sm">
                  Give the AI some context about you — your projects, interests, or working style.
                  This is optional.
                </p>
              </div>

              <Textarea
                autoFocus
                placeholder="e.g. I'm building a SaaS product and prefer concise, no-fluff answers. I like code examples over prose explanations."
                value={about}
                onChange={(e) => setAbout(e.target.value)}
                onKeyDown={handleKeyDown}
                className="min-h-[120px] text-sm resize-none"
              />

              <div className="flex gap-2">
                <Button variant="outline" onClick={() => setStep(2)} className="flex-1">
                  Back
                </Button>
                <Button
                  variant="ghost"
                  onClick={() => { setAbout(""); handleNext(); }}
                  className="flex-1 text-muted-foreground"
                >
                  Skip
                </Button>
                <Button className="flex-1 gap-2" onClick={handleNext}>
                  Continue
                  <ChevronRight className="w-4 h-4" />
                </Button>
              </div>
            </div>
          )}

          {/* ── Step 4: All set ── */}
          {step === 4 && (
            <div className="flex flex-col gap-6 animate-in fade-in slide-in-from-right-4 duration-300">
              <div className="flex flex-col items-center gap-3 text-center">
                <div className="w-14 h-14 rounded-2xl bg-green-500/10 border border-green-500/20 flex items-center justify-center">
                  <Sparkles className="w-7 h-7 text-green-400" />
                </div>
                <div>
                  <h2 className="text-2xl font-bold">You're all set{name ? `, ${name.trim()}` : ""}!</h2>
                  <p className="text-muted-foreground text-sm mt-1">
                    Here's what the AI knows about you.
                  </p>
                </div>
              </div>

              {/* Summary card */}
              <div className="bg-secondary/50 border border-border rounded-xl divide-y divide-border overflow-hidden">
                <ProfileRow icon={<User className="w-3.5 h-3.5" />} label="Name" value={name.trim()} fallback="Not set" />
                <ProfileRow icon={<Briefcase className="w-3.5 h-3.5" />} label="Role" value={profession.trim()} fallback="Not set" />
                <ProfileRow
                  icon={<BookOpen className="w-3.5 h-3.5" />}
                  label="About"
                  value={about.trim()}
                  fallback="Skipped"
                  multiline
                />
              </div>

              <p className="text-xs text-muted-foreground text-center">
                You can update this anytime in{" "}
                <span className="text-foreground font-medium">Settings → Profile</span>.
              </p>

              <Button autoFocus className="w-full gap-2" onClick={handleFinish} disabled={saving}>
                {saving ? (
                  "Saving…"
                ) : (
                  <>
                    <Check className="w-4 h-4" />
                    Start using XandSuite
                  </>
                )}
              </Button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function ProfileRow({
  icon,
  label,
  value,
  fallback,
  multiline,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  fallback: string;
  multiline?: boolean;
}) {
  return (
    <div className="flex items-start gap-3 px-4 py-3">
      <span className="text-muted-foreground mt-0.5 shrink-0">{icon}</span>
      <div className="flex-1 min-w-0">
        <div className="text-[11px] text-muted-foreground uppercase tracking-wide font-medium">{label}</div>
        <div className={cn("text-sm mt-0.5", !value && "text-muted-foreground italic", multiline && "whitespace-pre-wrap break-words")}>
          {value || fallback}
        </div>
      </div>
    </div>
  );
}
