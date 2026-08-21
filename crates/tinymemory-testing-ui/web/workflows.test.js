const assert = require("node:assert/strict");
const test = require("node:test");
const { uploadRequest, wireUpload } = require("./workflows.js");

class FakeFormData {
  constructor() { this.parts = []; }
  append(...part) { this.parts.push(part); }
}

test("upload request uses the document intake route and multipart fields", () => {
  const file = { name: "guide.md", size: 12 };
  const request = uploadRequest(file, {
    namespace: "manuals",
    category: "custom:guide",
    taint: "external_sync",
  }, () => new FakeFormData());
  assert.equal(request.path, "/documents/upload");
  assert.equal(request.options.method, "POST");
  assert.deepEqual(request.options.body.parts, [
    ["namespace", "manuals"],
    ["key", "guide.md"],
    ["category", "custom:guide"],
    ["taint", "external_sync"],
    ["file", file, "guide.md"],
  ]);
});

test("upload click wiring calls the tested request builder contract", async () => {
  let click;
  const calls = [];
  global.FormData = FakeFormData;
  wireUpload({
    button: { addEventListener(event, handler) { assert.equal(event, "click"); click = handler; } },
    filesInput: { files: [{ name: "note.txt", size: 4 }] },
    namespaceInput: { value: "notes" },
    categoryInput: { value: "daily" },
    taintInput: { value: "internal" },
    run: (workflow) => workflow(),
    call: async (...args) => calls.push(args),
  });
  await click();
  assert.equal(calls.length, 1);
  assert.equal(calls[0][0], "/documents/upload");
  assert.equal(calls[0][1].method, "POST");
  assert.deepEqual(calls[0][1].body.parts[0], ["namespace", "notes"]);
});
