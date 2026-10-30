const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const { test } = require('node:test');
const vm = require('node:vm');

// Exercise the actual recorder and startup code with DOM/IPC boundaries stubbed.
const html = readFileSync(join(__dirname, '../ui/index.html'), 'utf8');
const shortcutCode = html.split('// ── Keyboard shortcut ──')[1].split('// ── Compact mode ──')[0];

function setup(platform, saved) {
  const storage = new Map(saved ? [['arabizi-shortcut', saved]] : []);
  const listeners = new Map();
  const calls = [];
  const display = { innerHTML: '' };
  const context = vm.createContext({
    navigator: { platform },
    localStorage: {
      getItem: key => storage.get(key) ?? null,
      setItem: (key, value) => storage.set(key, value),
      removeItem: key => storage.delete(key),
    },
    document: {
      addEventListener: (type, handler) => listeners.set(type, handler),
      removeEventListener: type => listeners.delete(type),
    },
    shortcutCurrentEl: display,
    shortcutRecordBtn: { classList: { add() {}, remove() {} }, textContent: '' },
    invoke: async (command, args) => { calls.push({ command, ...args }); },
  });
  vm.runInContext(shortcutCode, context);
  return { context, storage, listeners, calls, display };
}

test('Mac default matches the native Command+Shift+A registration', async () => {
  const { context, display } = setup('MacIntel');
  await context.initShortcut();
  assert.equal(display.innerHTML.match(/<kbd>(.*?)<\/kbd>/g).join(''), '<kbd>Cmd</kbd><kbd>Shift</kbd><kbd>A</kbd>');
});

test('Windows default remains Control+Shift+A', async () => {
  const { context, display } = setup('Win32');
  await context.initShortcut();
  assert.match(display.innerHTML, /^<kbd>Ctrl<\/kbd>/);
});

test('custom Mac shortcuts preserve Command, including when it is the only modifier', async () => {
  for (const shiftKey of [false, true]) {
    const { context, storage, listeners, calls } = setup('MacIntel');
    context.startRecordingShortcut();
    listeners.get('keydown')({
      key: 'k', code: 'KeyK', metaKey: true, shiftKey,
      ctrlKey: false, altKey: false, preventDefault() {}, stopPropagation() {},
    });
    await new Promise(resolve => setImmediate(resolve));
    const expected = shiftKey ? 'Cmd+Shift+K' : 'Cmd+K';
    // Modifiers may be serialized in any order; both must survive recording.
    assert.deepEqual(calls[0].shortcutStr.split('+').sort(), expected.split('+').sort());
    assert.equal(storage.get('arabizi-shortcut'), calls[0].shortcutStr);
    assert.equal(listeners.size, 0);
  }
});

test('saved Control+Shift+A is restored on Mac even though it was the old displayed default', async () => {
  const { context, calls } = setup('MacIntel', 'Ctrl+Shift+A');
  await context.initShortcut();
  assert.equal(calls[0].command, 'update_shortcut');
  assert.equal(calls[0].shortcutStr, 'Ctrl+Shift+A');
});
