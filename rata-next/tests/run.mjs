/* Runs every suite against a production build. Assumes `npm run build` has
   already run — CI builds first, and so should you locally. */
const SUITES = [
  ['config route',       './config-route.test.mjs'],
  ['OAuth state binding', './oauth-state.test.mjs'],
  ['app (Chromium)',     './app.test.mjs'],
];

const state = { fails: 0 };
for (const [name, path] of SUITES) {
  console.log(`\n${'='.repeat(64)}\n${name}\n${'='.repeat(64)}`);
  const { default: run } = await import(path);
  try {
    await run(state);
  } catch (e) {
    console.log(`  FAIL  suite threw: ${e.message}`);
    state.fails++;
  }
}

console.log('\n' + '='.repeat(64));
console.log(state.fails ? `${state.fails} CHECK(S) FAILED` : 'All checks passed.');
process.exit(state.fails ? 1 : 0);
