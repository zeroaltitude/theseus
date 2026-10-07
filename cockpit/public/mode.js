// Night or daylight, before the first paint (theseus-hnof.5): the class lib/mode.ts keeps, read from the address or
// this browser, so a daylight page never flashes the night first. The choice is night, daylight, or the system's
// (theseus-001m), as lib/daylight.ts's choiceOf and modeOf read it. lib/mode.ts takes over once the app loads.
try {
  var ok = function (x) { return x === 'dark' || x === 'light' || x === 'system' }
  var asked = new URLSearchParams(location.search).get('mode')
  var kept = localStorage.getItem('cockpit.mode')
  var m = ok(asked) ? asked : ok(kept) ? kept : 'dark'
  if (m === 'system') m = window.matchMedia && window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark'
  if (m === 'light') { document.documentElement.classList.add('light'); document.documentElement.classList.remove('dark') }
} catch (e) { /* the night stands */ }
