// Run with: node --test tests/local-engine.cjs
// Checks the in-browser practice engine in web/app.js (the same rules the server enforces).
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

test('Evil Eye exposes one rival card to both players and leaves Sabotage alone', () => run(`
  const S=deal(0,0);
  assert.equal(okEye(S,1),false,'only on your own turn');
  assert.equal(okEye(S,0),true);
  doEye(S,0);
  const id=S.exposed[1];
  assert.ok(S.hands[1].includes(id),'the exposed card is in the rival hand');
  assert.equal(viewFor(S,0).ey,id,'the seer sees it');
  assert.equal(viewFor(S,1).em,id,'the victim learns which card');
  assert.equal(viewFor(S,0).em,-1);
  assert.deepEqual(viewFor(S,0).pw.eye,[0,1]);
  assert.equal(okEye(S,0),false,'once per battle');
  assert.equal(okSab(S,0),true,'Sabotage is independent of the Eye');
`));

test('the exposure ends when the card leaves the hand', () => run(`
  const S=deal(1,0);
  assert.equal(okEye(S,1),true);doEye(S,1);const id=S.exposed[0];
  S.hands[0].splice(S.hands[0].indexOf(id),1);
  assert.equal(viewFor(S,1).ey,-1);
  assert.equal(viewFor(S,0).em,-1);
`));

test('the Eye is locked when fewer than 10 cards are left in the deck', () => run(`
  const S=deal(0,0);S.deck.splice(0,S.deck.length-9);
  assert.equal(okEye(S,0),false);
`));

test('Sabotage knocks a rival card to the discards and they draw a replacement', () => run(`
  const S=deal(0,0),n=S.hands[1].length,d=S.deck.length;
  assert.equal(okSab(S,0),true);doSab(S,0);
  assert.equal(S.hands[1].length,n);
  assert.equal(S.deck.length,d-1);
  assert.equal(S.disc.flat().length,1);
  assert.equal(okSab(S,0),false,'once per battle');
  assert.equal(okEye(S,0),true,'the Eye is still available');
`));

test('Rush is a secret: the rival is not told until their turn', () => run(`
  const S=deal(0,0);
  assert.equal(okRush(S,0),true);doRush(S,0);
  const v=viewFor(S,1);
  assert.equal(v.rn,null,'no pending-rush marker for the target');
  assert.equal(v.la&&v.la.k,'none','the rush itself is masked');
  assert.deepEqual(v.pw.rush,[1,1],'their view does not show it as used');
  assert.equal(viewFor(S,0).la.k,'rush','the rusher sees their own rush');
`));
