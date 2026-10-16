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
Calendar conversion, date parsing/formatting and named timezones are later
extensions, not promises of these two handlers.

Link this module in a browser shell as well as in its guests; the compiler
includes page.mjs in generated host.mjs when it is linked. No core import or
native module ABI changes are needed.
