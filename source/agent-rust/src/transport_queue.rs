//! Bounded transport queues. Control frames never wait behind queued bulk data.
use tokio::sync::mpsc;
pub struct Sender<T> {
    control: mpsc::Sender<T>,
    bulk: mpsc::Sender<T>,
}
impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            control: self.control.clone(),
            bulk: self.bulk.clone(),
        }
    }
}
pub struct Receiver<T> {
    control: mpsc::Receiver<T>,
    bulk: mpsc::Receiver<T>,
}
pub fn channel<T>(control: usize, bulk: usize) -> (Sender<T>, Receiver<T>) {
    let (c, cr) = mpsc::channel(control);
    let (b, br) = mpsc::channel(bulk);
    (
        Sender {
            control: c,
            bulk: b,
        },
        Receiver {
            control: cr,
            bulk: br,
        },
    )
}
impl<T> Sender<T> {
    pub fn try_control(&self, value: T) -> Result<(), mpsc::error::TrySendError<T>> {
        self.control.try_send(value)
    }
    pub fn try_bulk(&self, value: T) -> Result<(), mpsc::error::TrySendError<T>> {
        self.bulk.try_send(value)
    }
    pub async fn send_bulk(&self, value: T) -> Result<(), mpsc::error::SendError<T>> {
        self.bulk.send(value).await
    }
}
impl<T> Receiver<T> {
    pub async fn recv(&mut self) -> Option<T> {
        tokio::select! {
            biased;
            Some(message) = self.control.recv() => Some(message),
            Some(message) = self.bulk.recv() => Some(message),
            else => None,
        }
    }
}
/// Close stays ordered after data from the same tunnel; prioritizing it would
/// discard already queued SSH bytes. Only independent control may overtake data.
pub fn bulk(kind: &str) -> bool {
    matches!(kind, "ssh_data" | "ssh_close")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn full_bulk_queue_does_not_block_heartbeat_and_ack() {
        let (out, mut rx) = channel(2, 2);
        out.try_bulk("data1").unwrap();
        out.try_bulk("data2").unwrap();
        assert!(out.try_bulk("data3").is_err());
        out.try_control("ping").unwrap();
        out.try_control("ack").unwrap();
        assert!(out.try_control("overflow").is_err());
        assert_eq!(rx.recv().await, Some("ping"));
        assert_eq!(rx.recv().await, Some("ack"));
        assert_eq!(rx.recv().await, Some("data1"));
        assert_eq!(rx.recv().await, Some("data2"));
    }
    #[tokio::test]
    async fn queued_data_and_close_drain_after_all_senders_drop() {
        let (out, mut rx) = channel(1, 3);
        out.send_bulk("data1").await.unwrap();
        out.send_bulk("data2").await.unwrap();
        out.send_bulk("close").await.unwrap();
        drop(out);
        assert_eq!(rx.recv().await, Some("data1"));
        assert_eq!(rx.recv().await, Some("data2"));
        assert_eq!(rx.recv().await, Some("close"));
        assert_eq!(rx.recv().await, None);
    }
    #[test]
    fn close_preserves_byte_order_and_control_has_separate_capacity() {
        for name in ["ssh_data", "ssh_close"] {
            assert!(bulk(name));
        }
        for name in [
            "metrics",
            "ack",
            "ping",
            "pong",
            "ssh_ack",
            "ssh_ready",
            "ssh_open",
        ] {
            assert!(!bulk(name));
        }
    }
}
