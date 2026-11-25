
#[async_trait]
impl EventHandler for Receive {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        match ctx {
            EventContext::SpeakingStateUpdate(speaking) => {
                if let Some(user) = speaking.user_id {
                    let mut ssrcs = self.ssrcs.lock().expect("the SSRC map's lock");
                    ssrcs.speaking(speaking.ssrc, Speaker(user.0));
                }
            }
            EventContext::VoiceTick(tick) => {
                let packets = tick.speaking.iter().map(|(ssrc, data)| {
                    let arrived = data.packet.is_some();
                    (*ssrc, data.decoded_voice.as_deref(), arrived)
                });
                let (frames, steady) = {
                    let mut ssrcs = self.ssrcs.lock().expect("the SSRC map's lock");
                    (ssrcs.tick(packets), ssrcs.steady)
                };
                if steady {
                    // Once: a watch that already says so is left alone.
                    self.flows.send_if_modified(|f| !std::mem::replace(f, true));
                }
                let _ = self.tx.send(Signal::Heard(Heard::Tick(frames)));
            }
            EventContext::RtcpPacket(r) => {
                if let RtcpPacket::SenderReport(s) = r.rtcp() {
                    // The sender info: NTP time (8 bytes), RTP time (4), then
                    // the packets sent (4), big-endian (RFC 3550, 6.4.1).
                    let sent = s
                        .payload()
                        .get(12..16)
                        .and_then(|b| b.try_into().ok())
                        .map(u32::from_be_bytes);
                    let mut ssrcs = self.ssrcs.lock().expect("the SSRC map's lock");
                    ssrcs.report(s.get_ssrc(), sent, Instant::now());
                }
            }
            EventContext::ClientDisconnect(gone) => {
                let mut ssrcs = self.ssrcs.lock().expect("the SSRC map's lock");
                ssrcs.left(Speaker(gone.user_id.0));
            }
            EventContext::DriverDisconnect(d) => {
                // A rejoin's own leave: the call goes on.
                if self.rejoining.load(Ordering::SeqCst)
                    && d.reason == Some(DisconnectReason::Requested)
                {
                    return None;
                }
                let why = match d.reason {
                    Some(r) => format!("{:?}: {r:?}", d.kind),
                    None => format!("{:?}", d.kind),
                };
                let _ = self.tx.send(Signal::Gone(why));
            }
            _ => {}
        }
        None
    }
}
