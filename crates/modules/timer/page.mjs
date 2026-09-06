// The page's half of `timer`: a particle, later.
(ctx) => {
  // Kept so `Cancel` has something to name. A fired delay drops out of it —
  // nothing repeats on its own, so nothing here outlives its own firing.
  const pending = new Map();
  let next = 1;

  return [
    "timer",
    (particle) => {
      switch (particle._class) {
        case "Delay": {
          // The application names the class it wants back, in advance. What
          // it hands over is the particle itself, so a delay can carry
          // whatever the handler will need — not one field's worth.
          const then = particle.then;
          const later =
            typeof then === "string" && then
              ? { _class: then }
              : then && typeof then === "object" && typeof then._class === "string"
                ? then
                : null;
          // Nothing to fire: a delay whose particle nobody named would spend
          // the wait and then have nowhere to go.
          if (!later) return { _class: "DelayResult", value: null };

          // No `ms` means as soon as the page is between things again — the
          // shortest wait there is, and a useful one for handing work back.
          const ms = typeof particle.ms === "number" && particle.ms > 0 ? particle.ms : 0;
          const id = next++;
          const handle = setTimeout(() => {
            pending.delete(id);
            ctx.fire(later);
          }, ms);
          pending.set(id, handle);
          return { _class: "DelayResult", value: id };
        }

        case "Cancel": {
          const handle = pending.get(particle.id);
          // Cancelling one that has already fired, or was never started, is
          // false rather than a failure: it means the same thing either way.
          if (handle === undefined) return { _class: "CancelResult", ok: false };
          clearTimeout(handle);
          pending.delete(particle.id);
          return { _class: "CancelResult", ok: true };
        }

        default:
          return null;
      }
    },
  ];
}
