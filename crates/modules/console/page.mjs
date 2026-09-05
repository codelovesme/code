// The page's half of `console`: where a line goes when there is no stdout.
//
// One function, and the smallest half of any module here — which is the
// point. The rendering, the counting, the decision about what a value looks
// like as text all happen in the module; this only says where the finished
// line lands.
(ctx) => {
  const { str, log } = ctx;
  return {
    code_web_log: (ptr, len) => log(str(ptr, len)),
  };
}
