// Run with: node --test tests/local-engine.cjs
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(require('node:path').join(__dirname, '../web/app.js'), 'utf8');
const engine = source.slice(source.indexOf('const rid='), source.indexOf('/* ================= Computer opponent'));
const helpers = source.slice(source.indexOf('const cC='), source.indexOf('const $='));
function run(code) {
  vm.runInNewContext(helpers + '\nconst clone=o=>JSON.parse(JSON.stringify(o));\n' + engine + '\n' + code, { assert, Date, Math });
}

test('Evil Eye reveals one private snapshot, survives play, and expires after draw', () => run(`
  const S=deal(0,0), before=S.hands[1].slice(), seq=S.seq;
  assert.equal(viewFor(S,0).peek,null);
  assert.equal(okEye(S,1),false);
  assert.equal(doEye(S,1),false);
  assert.equal(doEye(S,0),true);
  assert.equal(S.seq,seq+1);
  assert.deepEqual(viewFor(S,0).peek,before);
  assert.equal(viewFor(S,1).peek,null);
  assert.equal(doEye(S,0),false);
  doSab(S,0);
  assert.deepEqual(viewFor(S,0).peek,before);
  assert.notDeepEqual(S.hands[1],before);
  doPlay(S,0,S.hands[0][0],false);
  assert.deepEqual(viewFor(S,0).peek,before);
  doDraw(S,0,-1);
  assert.equal(viewFor(S,0).peek,null);
  assert.equal(doEye(S,1),true);
  assert.deepEqual(viewFor(S,1).peek,S.hands[0]);
  assert.equal(viewFor(S,0).peek,null);
  doFlag(S,1,'forfeit');
  assert.equal(viewFor(S,1).peek,null);
  assert.equal(doEye(S,0),false);
`));

test('Evil Eye rejects draw phase and clears on automatic turn completion', () => run(`
  const S=deal(0,0);
  doPlay(S,0,S.hands[0][0],false);
  assert.equal(doEye(S,0),false);
  assert.equal(S.pw.eye[0],1);
  doDraw(S,0,-1);
  assert.equal(doEye(S,1),true);
  autoMove(S,1);
  assert.equal(viewFor(S,1).peek,null);
  assert.equal(S.peek[1],null);
`));
