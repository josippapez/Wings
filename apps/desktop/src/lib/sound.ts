/** The sound for a new notification or toast: a short tone Web Audio makes on the spot, so no files ship. */
export type SoundId = "chime" | "pop" | "ping" | "drop";
export type SoundSettings = { enabled: boolean; sound: SoundId };
export const DEFAULT_SOUND: SoundSettings = { enabled: true, sound: "chime" };

/** Notes of a sound: frequency in Hz, start and length in seconds. */
type Note = { hz: number; at: number; len: number };
export const SOUNDS: { id: SoundId; label: string; notes: Note[] }[] = [
  { id: "chime", label: "Chime", notes: [{ hz: 880, at: 0, len: 0.25 }, { hz: 1318.5, at: 0.09, len: 0.35 }] },
  { id: "pop", label: "Pop", notes: [{ hz: 520, at: 0, len: 0.1 }] },
  { id: "ping", label: "Ping", notes: [{ hz: 1568, at: 0, len: 0.5 }] },
  { id: "drop", label: "Drop", notes: [{ hz: 660, at: 0, len: 0.12 }, { hz: 440, at: 0.1, len: 0.22 }] },
];

/** The saved setting, with anything missing or unknown replaced by the default. */
export function cleanSound(saved: unknown): SoundSettings {
  const s = saved && typeof saved === "object" ? (saved as Record<string, unknown>) : {};
  return {
    enabled: typeof s.enabled === "boolean" ? s.enabled : DEFAULT_SOUND.enabled,
    sound: SOUNDS.some((x) => x.id === s.sound) ? (s.sound as SoundId) : DEFAULT_SOUND.sound,
  };
}

/** Notifications this close together share one sound. */
export const BURST_MS = 1000;

/** Returns a function that says whether a sound is due at `now` (ms): the first one is, then none until `windowMs` has passed. */
export function burstGate(windowMs = BURST_MS) {
  let last = -Infinity;
  return (now: number) => {
    if (now - last < windowMs) return false;
    last = now;
    return true;
  };
}

let ctx: AudioContext | null = null;

/** Plays one sound now. A webview that has blocked audio plays nothing. */
export function playSound(id: SoundId) {
  const notes = SOUNDS.find((s) => s.id === id)?.notes;
  if (!notes || typeof AudioContext === "undefined") return;
  ctx ??= new AudioContext();
  void ctx.resume();
  const t0 = ctx.currentTime;
  for (const { hz, at, len } of notes) {
    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    osc.type = "sine";
    osc.frequency.value = hz;
    // A quick attack and a smooth fade keep it from clicking.
    gain.gain.setValueAtTime(0.0001, t0 + at);
    gain.gain.exponentialRampToValueAtTime(0.18, t0 + at + 0.01);
    gain.gain.exponentialRampToValueAtTime(0.0001, t0 + at + len);
    osc.connect(gain).connect(ctx.destination);
    osc.start(t0 + at);
    osc.stop(t0 + at + len + 0.02);
  }
}

let current = DEFAULT_SOUND;
const due = burstGate();

/** App keeps this up to date with the setting. */
export function setSoundSettings(settings: SoundSettings) {
  current = settings;
}

/** A notification or toast came in: plays the chosen sound unless it's off or one just played. */
export function notificationSound() {
  if (current.enabled && due(Date.now())) playSound(current.sound);
}
