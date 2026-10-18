(_ctx) => {
  // Hinnant's days_from_civil / civil_from_days, as the native half: whole
  // numbers, month 1–12, day 1–31 rolling past the month's end.
  const whole = (p, name, cls, limit) => {
    const n = p[name];
    if (typeof n !== "number" || !Number.isFinite(n) || !Number.isInteger(n) || Math.abs(n) > limit) throw `${cls} requires a whole-number '${name}'`;
    return n;
  };
  const daysFromDate = (p) => {
    let y = whole(p, "year", "DaysFromDate", 1e8);
    const m = whole(p, "month", "DaysFromDate", 1e8), d = whole(p, "day", "DaysFromDate", 1e8);
    if (m < 1 || m > 12) throw "DaysFromDate requires a 'month' from 1 to 12";
    if (d < 1 || d > 31) throw "DaysFromDate requires a 'day' from 1 to 31";
    if (m <= 2) y -= 1;
    const era = Math.floor(y / 400), yoe = y - era * 400;
    const doy = Math.floor((153 * (m > 2 ? m - 3 : m + 9) + 2) / 5) + d - 1;
    const doe = yoe * 365 + Math.floor(yoe / 4) - Math.floor(yoe / 100) + doy;
    return era * 146097 + doe - 719468;
  };
  const dateFromDays = (days) => {
    const z = days + 719468, era = Math.floor(z / 146097), doe = z - era * 146097;
    const yoe = Math.floor((doe - Math.floor(doe / 1460) + Math.floor(doe / 36524) - Math.floor(doe / 146096)) / 365);
    const doy = doe - (365 * yoe + Math.floor(yoe / 4) - Math.floor(yoe / 100));
    const mp = Math.floor((5 * doy + 2) / 153), d = doy - Math.floor((153 * mp + 2) / 5) + 1;
    const m = mp < 10 ? mp + 3 : mp - 9;
    return { year: yoe + era * 400 + (m <= 2 ? 1 : 0), month: m, day: d, weekday: ((days + 4) % 7 + 7) % 7 };
  };
  const fail = (message) => ({ _class: "Exception", source: "time", message, innerException: null });
  return ["time", (particle) => {
    switch (particle._class) {
      case "Timestamp": return { _class: "TimestampResult", value: Math.floor(Date.now() / 1000) };
      case "TimezoneOffset": return { _class: "TimezoneOffsetResult", value: -new Date().getTimezoneOffset() || 0 };
      case "DaysFromDate": try { return { _class: "DaysFromDateResult", value: daysFromDate(particle) || 0 }; } catch (e) { return fail(e); }
      case "DateFromDays": try { const days = whole(particle, "days", "DateFromDays", 1e11); return { _class: "DateFromDaysResult", ...dateFromDays(days) }; } catch (e) { return fail(e); }
      default: return null;
    }
  }];
}
