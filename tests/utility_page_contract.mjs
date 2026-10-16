// Direct host-value edge cases supplement the actual wasm/native fixtures.
import { readFileSync } from 'node:fs';
import { runInThisContext } from 'node:vm';
import assert from 'node:assert/strict';
const root = new URL('../crates/modules/', import.meta.url);
const dispatch = (name) => runInThisContext('(' + readFileSync(new URL(name + '/page.mjs', root), 'utf8') + ')')({})[1];
const math = dispatch('math'), strings = dispatch('strings'), time = dispatch('time');
const error = (particle, message) => assert.deepEqual(math(particle), {
  _class: 'Exception', source: 'math', message, innerException: null,
});
for (const value of [NaN, Infinity, -Infinity, null, true, '2']) {
  error({ _class: 'Floor', value }, "Floor requires a numeric 'value'");
  error({ _class: 'Pow', value: 2, exponent: value }, "Pow requires a numeric 'exponent'");
  error({ _class: 'Sum', value: [1, value] }, 'Sum requires an array of numbers');
}
error({ _class: 'Double', value: Number.MAX_VALUE }, 'Double result must be finite');
error({ _class: 'Sum', value: [Number.MAX_VALUE, Number.MAX_VALUE] }, 'Sum result must be finite');
error({ _class: 'Div', value: Number.MAX_VALUE, by: Number.MIN_VALUE }, 'Div result must be finite');
for (const text of ['1\n', '1\r\n', ' 1', '1 ', '0x20', 'NaN', 'Infinity', '', '+', '.']) {
  error({ _class: 'Parse', text }, 'Parse requires decimal text');
}
for (const [n, wanted] of [[-2.5, -3], [-0.5, -1], [-0.49999999999999994, 0], [0.49999999999999994, 0], [2.5, 3]]) {
  assert.equal(math({ _class: 'Round', value: n }).value, wanted);
}
for (const name of ['Round', 'Trunc', 'Sin', 'Abs']) {
  assert.equal(Object.is(math({ _class: name, value: -0 }).value, -0), false);
}
assert.equal(strings({ _class: 'Length', value: 'a😀é' }).value, 3);
assert.equal(strings({ _class: 'Length', value: 'e\u0301' }).value, 2);
assert.equal(strings({ _class: 'Upper', value: 'éclair' }).value, 'éCLAIR');
assert.equal(strings({ _class: 'Trim', value: '\u00a0 x \u00a0' }).value, '\u00a0 x \u00a0');
assert.deepEqual(strings({ _class: 'Split', value: 'a😀😀b', separator: '😀' }).value, ['a', '', 'b']);
assert.equal(strings({ _class: 'Join', value: ['a', 'b'], separator: '😀' }).value, 'a😀b');
const RealDate = Date;
try {
  globalThis.Date = class extends RealDate {
    static now() { return 1790000000123; }
    getTimezoneOffset() { return -330; }
  };
  assert.deepEqual(time({ _class: 'Timestamp' }), { _class: 'TimestampResult', value: 1790000000 });
  assert.deepEqual(time({ _class: 'TimezoneOffset' }), { _class: 'TimezoneOffsetResult', value: 330 });
} finally { globalThis.Date = RealDate; }
assert.equal(math({ _class: 'Unknown' }), null);
assert.equal(strings({ _class: 'Unknown' }), null);
assert.equal(time({ _class: 'Unknown' }), null);
console.log('utility browser edge contracts passed');
// Optional fields distinguish absence from explicit null; all integer checks
// must match the native ABI even for JS values JSON cannot represent.
for (const n of [NaN, Infinity, -Infinity, 0.5, 9007199254740992, null, true]) {
  for (const [name, key] of [['Pad', 'width'], ['Slice', 'start'], ['Slice', 'end']]) {
    assert.deepEqual(strings({ _class: name, value: 'a😀é', [key]: n }), {
      _class: 'Exception', source: 'strings', innerException: null,
      message: `${name} requires a safe integer '${key}' field`,
    });
  }
}
const million = strings({ _class: 'Pad', value: '😀', width: 1000000, fill: 'é😀' }).value;
assert.equal([...million].length, 1000000);
assert.equal(million.endsWith('😀'), true);
assert.equal(strings({ _class: 'Pad', value: '', width: 1000001 })._class, 'Exception');
assert.equal(strings({ _class: 'Slice', value: '😀é', start: -0, end: 9007199254740991 }).value, '😀é');
assert.equal(strings({ _class: 'Replace', value: 'aaa', search: 'aa', replacement: '$&' }).value, '$&a');
