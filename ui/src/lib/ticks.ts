import { addDays, addHours, addMonths, format, startOfDay, startOfHour, startOfMonth, startOfWeek } from 'date-fns';

export interface Tick { t: number; label: string; major: boolean }

/** Axis ticks for [from, to] (ms) at the given scale, in the local time zone. Density adapts to the continuous scale. */
export function ticks(ppd: number, from: number, to: number): Tick[] {
  const out: Tick[] = [];
  if (ppd >= 500) {
    const stepH = ppd >= 1500 ? 1 : ppd >= 900 ? 2 : 3;
    for (let d = startOfHour(from); d.getTime() <= to; d = addHours(d, 1)) {
      const h = d.getHours();
      if (h % stepH !== 0) continue;
      out.push({ t: d.getTime(), label: h === 0 ? format(d, 'EEE d MMM') : format(d, 'HH:mm'), major: h === 0 });
    }
  } else if (ppd >= 70) {
    for (let d = startOfDay(from); d.getTime() <= to; d = addDays(d, 1)) {
      out.push({ t: d.getTime(), label: format(d, d.getDate() === 1 ? 'd MMM' : 'EEE d'), major: d.getDay() === 1 || d.getDate() === 1 });
    }
  } else {
    for (let d = startOfMonth(from); d.getTime() <= to; d = addMonths(d, 1)) {
      out.push({ t: d.getTime(), label: format(d, d.getMonth() === 0 ? 'MMM yyyy' : 'MMMM'), major: true });
    }
    for (let d = startOfWeek(from, { weekStartsOn: 1 }); d.getTime() <= to; d = addDays(d, 7)) {
      out.push({ t: d.getTime(), label: '', major: false });
    }
  }
  return out;
}
