// Night or daylight, before the first paint (theseus-hnof.5): the class lib/mode.ts keeps, read from the address or
// this browser, so a daylight page never flashes the night first. lib/mode.ts takes over once the app loads.
try {
  var m = new URLSearchParams(location.search).get('mode') || localStorage.getItem('cockpit.mode')
  if (m === 'light') { document.documentElement.classList.add('light'); document.documentElement.classList.remove('dark') }
} catch (e) { /* the night stands */ }
