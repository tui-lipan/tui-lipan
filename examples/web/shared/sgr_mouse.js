export function parseSgrMouse(data) {
  const m = data.match(/^\x1b\[<(\d+);(\d+);(\d+)([Mm])$/);
  if (!m) {
    return null;
  }

  const [, bStr, xStr, yStr, suffix] = m;
  const b = Number.parseInt(bStr, 10);
  const x = Math.max(0, Number.parseInt(xStr, 10) - 1);
  const y = Math.max(0, Number.parseInt(yStr, 10) - 1);
  const button = b & 0b11;
  const isDrag = (b & 32) !== 0;
  const isWheel = (b & 64) !== 0;
  const phase = suffix === "m" ? 1 : isDrag ? 2 : 0;

  return {
    x,
    y,
    button,
    phase,
    isWheel,
    shift: (b & 4) !== 0,
    alt: (b & 8) !== 0,
    ctrl: (b & 16) !== 0,
  };
}
