use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::TrackLocalWriter;
use webrtc::track::track_remote::TrackRemote;

/// Track forwarder - reads RTP from remote track and writes to local track
pub struct TrackForwarder {
    remote_track: Arc<TrackRemote>,
    local_track: Arc<TrackLocalStaticRTP>,
    running: AtomicBool,
}

impl TrackForwarder {
    pub fn new(remote_track: Arc<TrackRemote>, local_track: Arc<TrackLocalStaticRTP>) -> Self {
        Self {
            remote_track,
            local_track,
            running: AtomicBool::new(false),
        }
    }

    /// Start forwarding RTP packets
    pub async fn start(&self) {
        if self.running.swap(true, Ordering::SeqCst) {
            return; // Already running
        }

        let remote_track = self.remote_track.clone();
        let local_track = self.local_track.clone();
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        tokio::spawn(async move {
            while running_clone.load(Ordering::SeqCst) {
                // Read RTP packet from remote track
                match remote_track.read_rtp().await {
                    Ok((rtp_packet, _attributes)) => {
                        // Write RTP packet to local track for forwarding
                        if let Err(e) = local_track.write_rtp(&rtp_packet).await {
                            tracing::trace!(error = %e, "Error writing RTP to local track");
                            // Don't break on write errors, just continue
                        }
                    }
                    Err(e) => {
                        // Check if it's just a timeout or if we should stop
                        if running_clone.load(Ordering::SeqCst) {
                            tracing::trace!(error = %e, "Error reading RTP from remote track");
                        }
                        break;
                    }
                }
            }

            tracing::debug!("Track forwarder stopped");
        });
    }

    /// Stop forwarding
    pub async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Check if forwarder is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}
