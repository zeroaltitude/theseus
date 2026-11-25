
impl WalFollower {

    fn read_into(
        &mut self,
        max_bytes: usize,
        upto: u64,
        batch: &mut Batch,
    ) -> Result<(), FollowError> {
        let Some((mut seg, mut off)) = self.reading_from()? else {
            self.stop = Stop::CaughtUp;
            return Ok(());
        };
        let mut skips = Skips::default();
        loop {
            // Checked before this segment is read: once segment n+1 exists,
            // nothing more is written to n, so what is read of n is all of it.
            let next_exists = wal::segment_path(&self.dir, seg + 1).exists();
            let (file, len) = self.segment(seg, off)?;
            let budget = max_bytes.saturating_sub(batch.bytes as usize);
            let (window, capped) =
                read_window(&file, off, len, budget, (batch.is_empty(), self.skip_wrong))?;
            let span_from = off;
            let Frames {
                mut i,
                mut partial,
                held,
            } = self.read_frames(&window, (seg, off), upto, batch, &mut skips)?;
            // A skipped frame with no whole frame after it yet. When it was the last frame of a
            // sealed segment, the frame after it is the next segment's first: the skip is
            // carried there. Otherwise it is not skipped, and the cursor stays before it, so it
            // is read again once the frame after it is there.
            let carry = skips.from.is_some()
                && partial.is_none()
                && held.is_none()
                && !capped
                && next_exists;
            // A window the budget cut that held only skipped frames up to where it ended (or up to
            // the frame the cut went through: `capped` says the window cut the frame short, not
            // the log): read on from there, in the same segment, with the skips still pending.
            // Without this a run of wrong frames longer than the budget is met again, whole, for
            // ever. Only while the window made progress and the batch has room: a batch that
            // already met its budget would read an empty window for ever.
            if skips.from.is_some()
                && held.is_none()
                && capped
                && i > 0
                && (batch.bytes as usize) < max_bytes
            {
                off += i as u64;
                continue;
            }
            // Where a stop that abandons a skip is named: at the cursor, which is where the first
            // skipped frame began, in whatever segment that was.
            let mut stop_at = None;
            if let (Some((from_seg, from_at)), false) = (skips.from, carry) {
                skips.clear();
                self.resync = false;
                stop_at = Some((from_seg, from_at));
                // Back to where the first skipped frame began: inside this window, or (when the
                // skip was carried here) the offset in the segment it began at.
                i = match (from_seg == seg, from_at >= span_from) {
                    (true, true) => (from_at - span_from) as usize,
                    (true, false) => {
                        off = from_at;
                        0
                    }
                    (false, _) => 0,
                };
                if partial.is_none() && held.is_none() {
                    partial = Some("a wrong frame waits for the frame after it");
                }
            }
            if i > 0 {
                batch.spans.push((seg, span_from, off + i as u64));
                batch.ends.push(self.cursor.position);
            }
            off += i as u64;
            if let Some(h) = held {
                self.stop = h;
                return Ok(());
            }
            if let Some(reason) = partial {
                if capped {
                    // The window cut the frame short, not the log.
                    self.stop = Stop::Budget;
                } else if next_exists {
                    return Err(FollowError::Corrupt(format!(
                        "segment {seg}, offset {off}: {reason}, in a segment that has a successor"
                    )));
                } else {
                    let (segment, offset) = stop_at.unwrap_or((seg, off));
                    self.stop = Stop::Partial {
                        segment,
                        offset,
                        reason: reason.to_string(),
                    };
                }
                return Ok(());
            }
            if capped {
                self.stop = Stop::Budget;
                return Ok(());
            }
            // At the end of this segment's bytes.
            if !next_exists {
                self.stop = Stop::CaughtUp;
                return Ok(());
            }
            if seg > self.sealed_upto {
                batch.sealed.push(seg);
                self.sealed_upto = seg;
            }
            seg += 1;
            off = 0;
            if batch.bytes as usize >= max_bytes {
                // A skip carried this far has no whole frame after it in this batch.
                skips.clear();
                self.resync = false;
                self.stop = Stop::Budget;
                return Ok(());
            }
        }
    }
}
