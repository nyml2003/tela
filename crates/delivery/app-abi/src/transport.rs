//! Latest-wins frame transport over a reliable ordered channel.

use std::{collections::VecDeque, rc::Rc};

use tela_app_session::{AppFrameToken, RetainedTreeSnapshot};
use tela_contract::{FrameDamage, SemanticKey, UiFrame};

use crate::{FrameCodecError, decode_frame, decode_status, encode_frame, encode_status};

const TRANSPORT_MAGIC: [u8; 4] = *b"TLPT";
const TRANSPORT_VERSION: u16 = 1;
const TRANSPORT_HEADER_LEN: usize = TRANSPORT_MAGIC.len() + std::mem::size_of::<u16>();

/// A frame payload relative to a receiver-acknowledged sequence.
#[derive(Clone, Debug, PartialEq)]
pub enum FrameTransportPacket {
    /// Complete renderer state used for startup and window-expiry resynchronization.
    Snapshot {
        /// New monotonically increasing transport sequence.
        seq: u64,
        /// Application provenance token carried with the frame.
        token: AppFrameToken,
        /// Complete renderer input.
        frame: UiFrame,
        /// Repaint region relative to an empty/new backing target.
        damage: FrameDamage,
        /// Empty by definition: a snapshot replaces the entire retained tree.
        spine: Vec<SemanticKey>,
    },
    /// Repaint commands for `damage`, based on the receiver's retained `base_seq` image.
    Patch {
        /// Receiver sequence whose retained pixels this patch assumes.
        base_seq: u64,
        /// New monotonically increasing transport sequence.
        seq: u64,
        /// Application provenance token carried with the frame.
        token: AppFrameToken,
        /// Commands intersecting the supplied repaint region.
        frame: UiFrame,
        /// Repaint region to clear before drawing `frame`.
        damage: FrameDamage,
        /// Outermost retained tree coordinates replaced by this patch.
        spine: Vec<SemanticKey>,
    },
}

/// A transport packet plus the status projection from the same application publication.
#[derive(Clone, Debug, PartialEq)]
pub struct TransportPublication {
    /// Retained-frame update payload.
    pub packet: FrameTransportPacket,
    /// Non-drawing state valid with `packet.token()`.
    pub status: tela_app_session::AppStatus,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct WireTransportPublication {
    seq: u64,
    base_seq: Option<u64>,
    snapshot: bool,
    token: u64,
    frame: Vec<u8>,
    damage_flags: u8,
    damage_rects: Vec<WireRect>,
    spine: Vec<String>,
    status: Vec<u8>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct WireRect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

/// Encodes one acknowledged-base transport publication for a retained receiver.
pub fn encode_transport_publication(
    publication: &TransportPublication,
) -> Result<Vec<u8>, FrameCodecError> {
    if publication.status.frame_token != Some(publication.packet.token()) {
        return Err(FrameCodecError::Encode(
            "transport publication status token does not match packet token".to_owned(),
        ));
    }
    let (frame, damage, spine) = match &publication.packet {
        FrameTransportPacket::Snapshot {
            frame,
            damage,
            spine,
            ..
        } => {
            if !spine.is_empty() {
                return Err(FrameCodecError::Encode(
                    "transport snapshot cannot carry a retained tree spine".to_owned(),
                ));
            }
            (frame, damage, spine)
        }
        FrameTransportPacket::Patch {
            frame,
            damage,
            spine,
            ..
        } => {
            if spine.is_empty() {
                return Err(FrameCodecError::Encode(
                    "transport patch requires a retained tree spine".to_owned(),
                ));
            }
            (frame, damage, spine)
        }
    };
    let wire = WireTransportPublication {
        seq: publication.packet.sequence(),
        base_seq: publication.packet.base_sequence(),
        snapshot: publication.packet.is_snapshot(),
        token: publication.packet.token().get(),
        frame: encode_frame(frame)?,
        damage_flags: damage.flags.bits(),
        damage_rects: damage
            .rects
            .iter()
            .map(|rect| WireRect {
                x: rect.x,
                y: rect.y,
                w: rect.w,
                h: rect.h,
            })
            .collect(),
        spine: spine.iter().map(|key| key.0.clone()).collect(),
        status: encode_status(&publication.status)?,
    };
    let payload =
        postcard::to_allocvec(&wire).map_err(|error| FrameCodecError::Encode(error.to_string()))?;
    let mut bytes = Vec::with_capacity(TRANSPORT_HEADER_LEN + payload.len());
    bytes.extend_from_slice(&TRANSPORT_MAGIC);
    bytes.extend_from_slice(&TRANSPORT_VERSION.to_le_bytes());
    bytes.extend_from_slice(&payload);
    Ok(bytes)
}

/// Decodes one retained-frame transport publication.
pub fn decode_transport_publication(bytes: &[u8]) -> Result<TransportPublication, FrameCodecError> {
    if bytes.len() < TRANSPORT_HEADER_LEN || bytes[..4] != TRANSPORT_MAGIC {
        return Err(FrameCodecError::InvalidMagic);
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    if version != TRANSPORT_VERSION {
        return Err(FrameCodecError::UnsupportedVersion(version));
    }
    let (wire, remaining): (WireTransportPublication, &[u8]) =
        postcard::take_from_bytes(&bytes[TRANSPORT_HEADER_LEN..])
            .map_err(|error| FrameCodecError::Decode(error.to_string()))?;
    if !remaining.is_empty() {
        return Err(FrameCodecError::TrailingBytes);
    }
    let token = AppFrameToken::new(wire.token)
        .ok_or_else(|| FrameCodecError::Decode("transport token must be non-zero".to_owned()))?;
    let status = decode_status(&wire.status)?;
    if status.frame_token != Some(token) {
        return Err(FrameCodecError::Decode(
            "transport status token mismatch".to_owned(),
        ));
    }
    let flags = tela_contract::DirtyFlags::from_bits(wire.damage_flags)
        .ok_or_else(|| FrameCodecError::Decode("transport damage has unknown flags".to_owned()))?;
    let mut damage = FrameDamage::default();
    for rect in wire.damage_rects {
        if !rect.x.is_finite() || !rect.y.is_finite() || !rect.w.is_finite() || !rect.h.is_finite()
        {
            return Err(FrameCodecError::Decode(
                "transport damage rectangle must be finite".to_owned(),
            ));
        }
        damage.add_rect(
            tela_contract::Rect {
                x: rect.x,
                y: rect.y,
                w: rect.w,
                h: rect.h,
            },
            flags,
        );
    }
    damage.flags |= flags;
    let frame = decode_frame(&wire.frame)?;
    let packet = if wire.snapshot {
        if wire.base_seq.is_some() {
            return Err(FrameCodecError::Decode(
                "transport snapshot cannot have a base sequence".to_owned(),
            ));
        }
        if !wire.spine.is_empty() {
            return Err(FrameCodecError::Decode(
                "transport snapshot cannot carry a retained tree spine".to_owned(),
            ));
        }
        FrameTransportPacket::Snapshot {
            seq: wire.seq,
            token,
            frame,
            damage,
            spine: Vec::new(),
        }
    } else {
        let base_seq = wire.base_seq.ok_or_else(|| {
            FrameCodecError::Decode("transport patch requires a base sequence".to_owned())
        })?;
        if wire.spine.is_empty() {
            return Err(FrameCodecError::Decode(
                "transport patch requires a retained tree spine".to_owned(),
            ));
        }
        FrameTransportPacket::Patch {
            base_seq,
            seq: wire.seq,
            token,
            frame,
            damage,
            spine: wire.spine.into_iter().map(SemanticKey).collect(),
        }
    };
    Ok(TransportPublication { packet, status })
}

impl FrameTransportPacket {
    /// Monotonically increasing sequence assigned by the sender.
    pub fn sequence(&self) -> u64 {
        match self {
            Self::Snapshot { seq, .. } | Self::Patch { seq, .. } => *seq,
        }
    }

    /// Confirmed base sequence required by a patch, if any.
    pub fn base_sequence(&self) -> Option<u64> {
        match self {
            Self::Snapshot { .. } => None,
            Self::Patch { base_seq, .. } => Some(*base_seq),
        }
    }

    /// Whether this packet replaces the receiver's complete retained frame.
    pub fn is_snapshot(&self) -> bool {
        matches!(self, Self::Snapshot { .. })
    }

    /// Application publication token whose input provenance this transport state represents.
    pub fn token(&self) -> AppFrameToken {
        match self {
            Self::Snapshot { token, .. } | Self::Patch { token, .. } => *token,
        }
    }
}

/// One guest-local publication retained until it leaves the ACK window.
struct RetainedVersion {
    seq: u64,
    tree: Option<Rc<dyn RetainedTreeSnapshot>>,
}

/// Sender-side latest-wins window.
///
/// Each entry retains the immutable guest tree corresponding to its sequence. This is the
/// identity basis for a patch: a coordinate is valid when it exists in either the acknowledged
/// base or the candidate tree. The renderer only receives the encoded draw patch; it never sees
/// the guest tree or compares content.
pub struct FrameTransportSender {
    next_seq: u64,
    acked_seq: Option<u64>,
    retained: VecDeque<RetainedVersion>,
    window: usize,
}

impl FrameTransportSender {
    /// Creates a sender retaining at least one acknowledged base sequence.
    pub fn new(window: usize) -> Self {
        Self {
            next_seq: 0,
            acked_seq: None,
            retained: VecDeque::new(),
            window: window.max(1),
        }
    }

    /// Records the latest successfully applied receiver sequence. Older acknowledgements cannot
    /// move the base backward.
    pub fn acknowledge(&mut self, seq: u64) {
        if self.retained.iter().any(|entry| entry.seq == seq)
            && self.acked_seq.is_none_or(|acked| seq > acked)
        {
            self.acked_seq = Some(seq);
        }
    }

    /// Discards a publication that never reached presentation.
    ///
    /// The application transaction has already restored its graph dirty set at this point. The
    /// transport must likewise forget the unpublished tree so it cannot consume ACK-window
    /// capacity or become a base for a later patch.
    pub fn reject(&mut self, seq: u64) {
        self.retained.retain(|entry| entry.seq != seq);
        if self.acked_seq == Some(seq) {
            self.acked_seq = None;
        }
    }

    /// Produces the newest frame; intermediate unacknowledged states are never queued.
    pub fn publish(
        &mut self,
        token: AppFrameToken,
        frame: &UiFrame,
        damage: &FrameDamage,
        spine: &[SemanticKey],
        retained_tree: Option<Rc<dyn RetainedTreeSnapshot>>,
    ) -> FrameTransportPacket {
        self.next_seq = self
            .next_seq
            .checked_add(1)
            .expect("frame transport sequence exhausted");
        let seq = self.next_seq;
        self.retained.push_back(RetainedVersion {
            seq,
            tree: retained_tree.clone(),
        });
        while self.retained.len() > self.window {
            self.retained.pop_front();
        }
        if let Some(base_seq) = self.acked_seq.filter(|base_seq| {
            !spine.is_empty()
                && retained_tree
                    .as_ref()
                    .zip(
                        self.retained
                            .iter()
                            .find(|entry| entry.seq == *base_seq)
                            .and_then(|entry| entry.tree.as_ref()),
                    )
                    .is_some_and(|(candidate, base)| {
                        spine_patch_is_compatible(base, candidate, spine)
                    })
        }) {
            let mut patch = frame.clone();
            patch.commands.retain(|command| {
                damage
                    .rects
                    .iter()
                    .any(|rect| intersects(command.paint_bounds(), *rect))
            });
            return FrameTransportPacket::Patch {
                base_seq,
                seq,
                token,
                frame: patch,
                damage: damage.clone(),
                spine: spine.to_vec(),
            };
        }
        FrameTransportPacket::Snapshot {
            seq,
            token,
            frame: frame.clone(),
            damage: damage.clone(),
            spine: Vec::new(),
        }
    }
}

/// A structural patch is valid only when every declared dirty coordinate is present in the
/// acknowledged tree or in the candidate tree. `Some -> None` is a removal and `None -> Some`
/// an insertion; both are legitimate path patches. `None -> None` proves that the graph dirty
/// coordinate has no retained-tree meaning and must fall back to a snapshot.
fn spine_patch_is_compatible(
    base: &Rc<dyn RetainedTreeSnapshot>,
    candidate: &Rc<dyn RetainedTreeSnapshot>,
    spine: &[SemanticKey],
) -> bool {
    spine
        .iter()
        .all(|key| base.node_identity(key).is_some() || candidate.node_identity(key).is_some())
}

impl Default for FrameTransportSender {
    fn default() -> Self {
        Self::new(8)
    }
}

/// Receiver-side sequence guard. Actual pixels are maintained by the damage-aware renderer.
#[derive(Default)]
pub struct FrameTransportReceiver {
    applied_seq: Option<u64>,
}

/// One retained-frame update accepted by [`FrameTransportReceiver`].
#[derive(Clone, Debug, PartialEq)]
pub struct AppliedFrameTransport {
    /// Application provenance token carried by the packet.
    pub token: AppFrameToken,
    /// Full frame for snapshots, or commands limited to the retained patch damage.
    pub frame: UiFrame,
    /// Repaint region associated with `frame`.
    pub damage: FrameDamage,
    /// Replaced retained coordinates. Empty only for a complete snapshot.
    pub spine: Vec<SemanticKey>,
    /// Sender sequence now installed at the receiver.
    pub sequence: u64,
}

impl FrameTransportReceiver {
    /// Applies a packet only when its stated base is the receiver's real retained image.
    pub fn apply(
        &mut self,
        packet: FrameTransportPacket,
    ) -> Result<AppliedFrameTransport, Box<FrameTransportPacket>> {
        let valid = match &packet {
            FrameTransportPacket::Snapshot { .. } => true,
            FrameTransportPacket::Patch { base_seq, .. } => self.applied_seq == Some(*base_seq),
        };
        if !valid {
            return Err(Box::new(packet));
        }
        let (seq, token, frame, damage, spine) = match packet {
            FrameTransportPacket::Snapshot {
                seq,
                token,
                frame,
                damage,
                spine,
            }
            | FrameTransportPacket::Patch {
                seq,
                token,
                frame,
                damage,
                spine,
                ..
            } => (seq, token, frame, damage, spine),
        };
        self.applied_seq = Some(seq);
        Ok(AppliedFrameTransport {
            token,
            frame,
            damage,
            spine,
            sequence: seq,
        })
    }
}

fn intersects(a: tela_contract::Rect, b: tela_contract::Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, rc::Rc};

    use super::*;
    use tela_contract::{Color, DrawCommand, DrawPayload, Rect, UiFrame, Viewport};

    #[derive(Default)]
    struct TestTreeSnapshot(BTreeMap<SemanticKey, usize>);

    impl RetainedTreeSnapshot for TestTreeSnapshot {
        fn node_identity(&self, key: &SemanticKey) -> Option<usize> {
            self.0.get(key).copied()
        }
    }

    fn tree(
        entries: impl IntoIterator<Item = (&'static str, usize)>,
    ) -> Rc<dyn RetainedTreeSnapshot> {
        Rc::new(TestTreeSnapshot(
            entries
                .into_iter()
                .map(|(key, identity)| (SemanticKey::from(key), identity))
                .collect(),
        ))
    }

    fn frame() -> UiFrame {
        UiFrame {
            viewport: Viewport {
                width: 100.0,
                height: 40.0,
            },
            commands: vec![
                DrawCommand {
                    geometry: Rect {
                        x: 0.0,
                        y: 0.0,
                        w: 20.0,
                        h: 20.0,
                    },
                    clip: None,
                    opacity: 1.0,
                    payload: DrawPayload::Rect {
                        fill: Some(Color::RED),
                        border: None,
                    },
                },
                DrawCommand {
                    geometry: Rect {
                        x: 70.0,
                        y: 0.0,
                        w: 20.0,
                        h: 20.0,
                    },
                    clip: None,
                    opacity: 1.0,
                    payload: DrawPayload::Rect {
                        fill: Some(Color::BLUE),
                        border: None,
                    },
                },
            ],
            hit_regions: vec![],
            scroll_bounds: vec![],
        }
    }
    #[test]
    fn patch_uses_ack_base_and_sends_only_damage_commands() {
        let mut sender = FrameTransportSender::new(2);
        let token = AppFrameToken::new(1).unwrap();
        let full = frame();
        let damage = FrameDamage::full(full.viewport, tela_contract::DirtyFlags::ALL);
        let first = sender.publish(token, &full, &damage, &[], Some(tree([("root", 1)])));
        let seq = match first {
            FrameTransportPacket::Snapshot { seq, .. } => seq,
            _ => panic!(),
        };
        sender.acknowledge(seq);
        let damage = FrameDamage {
            flags: tela_contract::DirtyFlags::VISUAL,
            rects: vec![Rect {
                x: 0.0,
                y: 0.0,
                w: 20.0,
                h: 20.0,
            }],
        };
        let spine = vec![SemanticKey::from("root/row")];
        match sender.publish(
            AppFrameToken::new(2).unwrap(),
            &full,
            &damage,
            &spine,
            Some(tree([("root/row", 2)])),
        ) {
            FrameTransportPacket::Patch {
                base_seq, frame, ..
            } => {
                assert_eq!(base_seq, seq);
                assert_eq!(frame.commands.len(), 1);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn empty_spine_forces_a_snapshot_even_after_an_acknowledgement() {
        let mut sender = FrameTransportSender::new(2);
        let token = AppFrameToken::new(1).expect("non-zero token");
        let full = frame();
        let damage = FrameDamage::full(full.viewport, tela_contract::DirtyFlags::ALL);
        let first = sender.publish(token, &full, &damage, &[], None);
        sender.acknowledge(first.sequence());

        let next = sender.publish(
            AppFrameToken::new(2).expect("non-zero token"),
            &full,
            &damage,
            &[],
            None,
        );
        assert!(next.is_snapshot());
        assert_eq!(next.base_sequence(), None);
    }

    #[test]
    fn missing_tree_snapshot_forces_a_snapshot_even_with_a_compatible_spine() {
        let mut sender = FrameTransportSender::new(2);
        let token = AppFrameToken::new(1).expect("non-zero token");
        let full = frame();
        let damage = FrameDamage::full(full.viewport, tela_contract::DirtyFlags::ALL);
        let first = sender.publish(token, &full, &damage, &[], Some(tree([("root", 1)])));
        sender.acknowledge(first.sequence());

        let spine = vec![SemanticKey::from("root")];
        let next = sender.publish(
            AppFrameToken::new(2).expect("non-zero token"),
            &full,
            &damage,
            &spine,
            None,
        );
        assert!(next.is_snapshot());
    }

    #[test]
    fn rejected_sequence_is_not_retained_as_a_future_patch_base() {
        let mut sender = FrameTransportSender::new(2);
        let full = frame();
        let damage = FrameDamage::full(full.viewport, tela_contract::DirtyFlags::ALL);
        let first = sender.publish(
            AppFrameToken::new(1).expect("non-zero token"),
            &full,
            &damage,
            &[],
            Some(tree([("root", 1)])),
        );
        sender.acknowledge(first.sequence());
        let rejected = sender.publish(
            AppFrameToken::new(2).expect("non-zero token"),
            &full,
            &damage,
            &[],
            Some(tree([("root", 2)])),
        );
        sender.reject(rejected.sequence());

        let spine = vec![SemanticKey::from("root")];
        let next = sender.publish(
            AppFrameToken::new(3).expect("non-zero token"),
            &full,
            &damage,
            &spine,
            Some(tree([("root", 3)])),
        );
        assert!(matches!(
            next,
            FrameTransportPacket::Patch { base_seq: 1, .. }
        ));
    }

    #[test]
    fn receiver_rejects_a_patch_without_its_acknowledged_base() {
        let packet = FrameTransportPacket::Patch {
            base_seq: 9,
            seq: 10,
            token: AppFrameToken::new(10).expect("non-zero token"),
            frame: frame(),
            damage: FrameDamage::full(
                Viewport {
                    width: 100.0,
                    height: 40.0,
                },
                tela_contract::DirtyFlags::ALL,
            ),
            spine: vec![SemanticKey::from("root")],
        };
        let rejected = FrameTransportReceiver::default()
            .apply(packet)
            .expect_err("patch cannot create its own retained base");
        assert_eq!(rejected.base_sequence(), Some(9));
    }

    #[test]
    fn transport_publication_round_trips_patch_metadata_and_status() {
        let token = AppFrameToken::new(3).expect("non-zero token");
        let publication = TransportPublication {
            packet: FrameTransportPacket::Patch {
                base_seq: 2,
                seq: 3,
                token,
                frame: frame(),
                damage: FrameDamage::full(
                    Viewport {
                        width: 100.0,
                        height: 40.0,
                    },
                    tela_contract::DirtyFlags::VISUAL,
                ),
                spine: vec![SemanticKey::from("root/dirty")],
            },
            status: tela_app_session::AppStatus {
                frame_token: Some(token),
                ..tela_app_session::AppStatus::default()
            },
        };
        let decoded = decode_transport_publication(
            &encode_transport_publication(&publication).expect("encode transport"),
        )
        .expect("decode transport");
        assert_eq!(decoded, publication);
    }
}
