import type { Archives } from "@/models/Archives";

const WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const FULL_MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

function pad(n: number): string {
  return String(n).padStart(2, "0");
}

function startOfDay(d: Date): number {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

export namespace Prettify {
  export function relativeDay(iso: string, now: Date = new Date()): string {
    const date = new Date(iso);
    const hm = `${pad(date.getHours())}:${pad(date.getMinutes())}`;
    const dayDiff = Math.round((startOfDay(now) - startOfDay(date)) / 86_400_000);
    if (dayDiff === 0) {
      return `today ${hm}`;
    }
    if (dayDiff === 1) {
      return `yesterday ${hm}`;
    }
    if (dayDiff === -1) {
      return `tomorrow ${hm}`;
    }
    if (dayDiff > 1 && dayDiff < 7) {
      return `${WEEKDAYS[date.getDay()]} ${hm}`;
    }
    const dm = `${date.getDate()} ${MONTHS[date.getMonth()]}`;
    return date.getFullYear() === now.getFullYear() ? `${dm} ${hm}` : `${dm} ${date.getFullYear()} ${hm}`;
  }

  export function fullDate(iso: string): string {
    const date = new Date(iso);
    return `${date.getDate()} ${FULL_MONTHS[date.getMonth()]} ${date.getFullYear()}, ${pad(date.getHours())}:${pad(date.getMinutes())}`;
  }

  export function ago(iso: string, now: Date = new Date()): string {
    const minutes = Math.round((now.getTime() - new Date(iso).getTime()) / 60_000);
    if (minutes < 1) {
      return "just now";
    }
    if (minutes < 60) {
      return `${minutes} min ago`;
    }
    const hours = Math.round(minutes / 60);
    if (hours < 24) {
      return `${hours} h ago`;
    }
    const days = Math.round(hours / 24);
    return `${days} ${days === 1 ? "day" : "days"} ago`;
  }

  export function duration(fromIso: string, toIso: string | null, now: Date = new Date()): string {
    const seconds = Math.max(0, Math.round(((toIso ? new Date(toIso) : now).getTime() - new Date(fromIso).getTime()) / 1000));
    if (seconds < 60) {
      return `${seconds} s`;
    }
    const minutes = Math.floor(seconds / 60);
    const rest = pad(seconds % 60);
    return minutes < 60 ? `${minutes}:${rest}` : `${Math.floor(minutes / 60)}:${pad(minutes % 60)}:${rest}`;
  }

  export function clock(iso: string): string {
    const d = new Date(iso);
    return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
  }

  export function retention(retention: Archives.Retention): string {
    const rules = [
      ["last", retention.keepLast],
      ["hourly", retention.keepHourly],
      ["daily", retention.keepDaily],
      ["weekly", retention.keepWeekly],
      ["monthly", retention.keepMonthly],
    ] as const;
    return `keep ${rules
      .filter(([, count]) => count > 0)
      .map(([name, count]) => `${name} ${count}`)
      .join(", ")}`;
  }

  export function isStale(iso: string, now: Date = new Date()): boolean {
    return now.getTime() - new Date(iso).getTime() > 2 * 86_400_000;
  }
}
