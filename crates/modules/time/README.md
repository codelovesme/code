# time — clock queries

Stateless first-party module for native and browser targets. Unreleased.

```code
link "time" as time
emit Timestamp to time get now
emit TimezoneOffset to time get local
```

| Handler | Result | Units |
|---|---|---|
| Timestamp {} | TimestampResult { value } | Whole Unix seconds |
| TimezoneOffset {} | TimezoneOffsetResult { value } | Minutes east of UTC, now |
| DaysFromDate { year, month, day } | DaysFromDateResult { value } | Days since 1970-01-01 |
| DateFromDays { days } | DateFromDaysResult { year, month, day, weekday } | weekday 0 = Sunday |

For example, an offset of +120 means add two hours to UTC to get local time.
The offset includes the running system/browser's current daylight-saving
rules. Timestamp is wall-clock time, not a monotonic duration clock.
Unknown classes answer null. No Config, pending work or serving lifecycle.

Native timezone queries use the same localtime_r convention as core on Unix;
on other native platforms the fallback is UTC, matching the interpreter.
Browser queries use Date.now()/1000 rounded down and the negated current
Date timezone offset. The result shapes match core's existing handlers.

`time` remains separate from `timer`: timer owns Delay/Cancel and pending
callbacks, whereas time only queries a clock. Core Timestamp/TimezoneOffset
remain compatible entry points so existing programs do not break. Core Length
also remains generic; its text-specific counterpart is strings Length.
The calendar is the proleptic Gregorian one, by Howard Hinnant's
days_from_civil / civil_from_days, with no timezone: a day number is a date,
not an instant. Fields are whole numbers (year within ±1e8, days within
±1e11); month is 1–12 and day 1–31, and a day past its month's end rolls
into the next month (2026-02-31 is 2026-03-03) — what the apps' hand-written
copies did. Anything else is an Exception from source "time". Both halves
compute the same numbers (`tests/time_calendar.code`). Date parsing and
formatting and named timezones remain later extensions.

Link this module in a browser shell as well as in its guests; the compiler
includes page.mjs in generated host.mjs when it is linked. No core import or
native module ABI changes are needed.
