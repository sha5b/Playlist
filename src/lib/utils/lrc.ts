export interface LrcLine {
	timeMs: number;
	text: string;
}

const LRC_LINE_RE = /^\[(\d{2}):(\d{2})(?:\.(\d{2,3}))?\](.*)/;

export function isLrcFormat(text: string): boolean {
	return LRC_LINE_RE.test(text.trim().split('\n')[0] ?? '');
}

export function parseLrc(text: string): LrcLine[] {
	const lines: LrcLine[] = [];
	for (const raw of text.split('\n')) {
		const m = raw.match(LRC_LINE_RE);
		if (!m) continue;
		const min = parseInt(m[1], 10);
		const sec = parseInt(m[2], 10);
		const ms = m[3] ? parseInt(m[3].padEnd(3, '0'), 10) : 0;
		lines.push({ timeMs: min * 60000 + sec * 1000 + ms, text: m[4].trim() });
	}
	return lines.sort((a, b) => a.timeMs - b.timeMs);
}

export function findActiveLine(lines: LrcLine[], positionMs: number): number {
	let idx = -1;
	for (let i = 0; i < lines.length; i++) {
		if (lines[i].timeMs <= positionMs) idx = i;
		else break;
	}
	return idx;
}
