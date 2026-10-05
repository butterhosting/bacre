import { describe, expect, it } from "bun:test";
import { Prettify } from "./Prettify";

describe("Prettify", () => {
  // Every moment is built on the local clock, as the helpers read it, so the tests hold in any time zone.
  // 3 October 2026 is a Saturday.
  const local = (year: number, month: number, day: number, hour: number, minute: number, second = 0) =>
    new Date(year, month - 1, day, hour, minute, second);
  const iso = (date: Date) => date.toISOString();
  const now = local(2026, 10, 3, 14, 20);

  describe("relativeDay", () => {
    const cases: Array<[string, Date, string]> = [
      ["earlier today", local(2026, 10, 3, 3, 0), "today 03:00"],
      ["later today", local(2026, 10, 3, 23, 59), "today 23:59"],
      ["yesterday, just before midnight", local(2026, 10, 2, 23, 58), "yesterday 23:58"],
      ["tomorrow", local(2026, 10, 4, 3, 0), "tomorrow 03:00"],
      ["earlier this week, by its weekday", local(2026, 9, 28, 3, 1), "Mon 03:01"],
      ["a week ago, by its date", local(2026, 9, 26, 3, 2), "26 Sep 03:02"],
      ["last year, with the year", local(2025, 12, 31, 23, 0), "31 Dec 2025 23:00"],
    ];
    for (const [name, date, expected] of cases) {
      it(`should say "${expected}" for a moment ${name}`, () => {
        expect(Prettify.relativeDay(iso(date), now)).toBe(expected);
      });
    }
  });

  it("should write the same full shape for any date", () => {
    expect(Prettify.fullDate(iso(local(2026, 10, 3, 16, 6)))).toBe("3 October 2026, 16:06");
    expect(Prettify.fullDate(iso(local(2026, 6, 9, 0, 5)))).toBe("9 June 2026, 00:05");
  });

  it("should round how long ago something was to the unit that reads best", () => {
    const ago = (minutes: number) => Prettify.ago(iso(new Date(now.getTime() - minutes * 60_000)), now);
    expect([ago(0.4), ago(3), ago(59), ago(60), ago(23 * 60), ago(24 * 60), ago(5 * 24 * 60)]).toEqual([
      "just now",
      "3 min ago",
      "59 min ago",
      "1 h ago",
      "23 h ago",
      "1 day ago",
      "5 days ago",
    ]);
  });

  it("should show a duration in seconds, then minutes, then hours", () => {
    const start = local(2026, 10, 3, 1, 0);
    const after = (seconds: number) => Prettify.duration(iso(start), iso(new Date(start.getTime() + seconds * 1000)));
    expect([after(41), after(127), after(3600 + 12 * 60 + 5)]).toEqual(["41 s", "2:07", "1:12:05"]);
    expect(Prettify.duration(iso(start), null, new Date(start.getTime() + 5000))).toBe("5 s");
    expect(Prettify.duration(iso(start), null, new Date(start.getTime() - 5000))).toBe("0 s");
  });

  it("should name only the retention rules that keep something", () => {
    const retention = { keepLast: 6, keepHourly: 24, keepDaily: 7, keepWeekly: 0, keepMonthly: 0 };
    expect(Prettify.retention(retention)).toBe("keep last 6, hourly 24, daily 7");
    expect(Prettify.retention({ ...retention, keepLast: 0, keepHourly: 0, keepMonthly: 12 })).toBe("keep daily 7, monthly 12");
  });

  it("should call a backup stale only after two full days", () => {
    const hoursAgo = (hours: number) => iso(new Date(now.getTime() - hours * 3_600_000));
    expect(Prettify.isStale(hoursAgo(47), now)).toBe(false);
    expect(Prettify.isStale(hoursAgo(49), now)).toBe(true);
  });
});
