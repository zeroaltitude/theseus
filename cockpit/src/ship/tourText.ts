// The tour's words (theseus-hnof): what each shape on the Ship is, in the order the tour shows them. Also the key's
// long form, and the owner's one page of "what you're looking at".

/** The tour's words, in order: also the key's long form, and the owner's one page. */
export const TOUR_TEXT: { title: string; body: string }[] = [
  { title: 'Your agent’s day, as a fleet', body: 'Each harbour is a place sessions come from: the CLI, the web UI, a DM, a channel. Its name and its sessions are written on its ring.' },
  { title: 'A ship is a session', body: 'One conversation. Its nameplate says what it is doing in plain words: working, waiting for you, failed, idle. Small boats in tow are the tasks it started.' },
  { title: 'A bench is a turn', body: 'The ship grows a bench across its deck for every turn, the oldest at the stern and the newest at the bow. The lamps on a bench are its message (ivory) and its model calls (violet).' },
  { title: 'An oar is a tool call', body: 'Each tool call is an oar of its turn’s bench. The blade is the result: green ok, rose with a cross failed, amber waiting for you, open while pending, a turning gear while a job runs. While a turn runs, its oars row.' },
  { title: 'The rig is the state', body: 'Sail up and a cyan rail: working. A lantern and an amber rail: waiting for you. A flare and a rose rail: failed or over budget. Brass at anchor: idle. A pennant stays on any bench where something failed.' },
  { title: 'The watch answers your questions', body: 'What is working now, what waits for you, what is slow against its usual, what today cost, what went wrong, and what happened since you last looked. Press show on a plate, or its key, 1 to 6, to light those ships and oars on the chart.' },
  { title: 'The sea is the work now', body: 'Dead calm when nothing happens: nothing on the chart moves unless something did. The swell rises with tokens a minute and the turns running, and settles as they end; the sea on the console says how high, in words. Calm mode stills it.' },
  { title: 'Zoom, and click anything', body: 'Scroll from the fleet to a ship, a turn, a call; the depth gauge says where you are. Hover anything for its card, click for its data. The ship’s log at the foot scrubs back through the day. Press ? for this tour again.' },
]

