(_ctx) => ["time", (particle) => {
  switch (particle._class) {
    case "Timestamp": return { _class: "TimestampResult", value: Math.floor(Date.now() / 1000) };
    case "TimezoneOffset": return { _class: "TimezoneOffsetResult", value: -new Date().getTimezoneOffset() || 0 };
    default: return null;
  }
}]
