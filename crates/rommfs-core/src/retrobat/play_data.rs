//! Merge RetroBat gamelist play data (favourites, last played, play count and
//! time) with RomM's. RomM derives play count and time from play sessions, so a
//! local play delta becomes one uploaded session per launch. The gamelist
//! `hidden` flag is not part of play-data sync; the "hide RomM copies" feature
//! owns it, so it is carried over verbatim.
//!
//! The rules never lose data: only the side that changed contributes, counts
//! and time add up, last played takes the later value, and a favourite conflict
//! resolves to the local side (the user's newest action is normally in
//! RetroBat). A failed push keeps the local delta for next time.
//! Pure functions; the caller supplies the clock and executes the push.

use crate::romm::{PlaySessionUpload, RemotePlayData};
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Play fields RomMFS owns on its own gamelist entries. `hidden` is
/// deliberately absent: it belongs to the "hide RomM copies" feature, not to
/// play-data sync, so it is carried over verbatim here.
pub const PLAY_FIELDS: [&str; 4] = ["favorite", "playcount", "lastplayed", "gametime"];
const LASTPLAYED_FORMAT: &str = "%Y%m%dT%H%M%S";

pub fn is_play_field(name: &str) -> bool {
    PLAY_FIELDS.contains(&name)
}

/// The play fields a gamelist carries for one of our entries.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct PlayValues {
    pub favorite: bool,
    #[serde(default)]
    pub playcount: u64,
    #[serde(default)]
    pub gametime_minutes: u64,
    #[serde(default)]
    pub lastplayed: Option<i64>,
}

/// The last reconciled local and remote values. Local is the gamelist side;
/// remote is what RomM held after the previous successful push.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct PlayBaseline {
    #[serde(default)]
    pub synced_local: PlayValues,
    #[serde(default)]
    pub synced_remote: PlayValues,
    /// False until the first reconciliation for this game. The first pass
    /// adopts the union of both sides rather than adding two histories that
    /// may already describe the same plays.
    #[serde(default)]
    pub known: bool,
}

/// Work to hand to RomM for one game. Empty means nothing changed locally.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlayPush {
    pub rom_id: i64,
    /// Desired favourite state, set only when the local value changed.
    pub favorite: Option<bool>,
    /// One session per local launch advance.
    pub sessions: Vec<PlaySessionUpload>,
}

impl PlayPush {
    pub fn is_empty(&self) -> bool {
        self.favorite.is_none() && self.sessions.is_empty()
    }
}

/// Result of reconciling one game's local and remote play data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayMerge {
    pub merged: PlayValues,
    pub push: PlayPush,
    /// Baseline once `push` succeeds (identity when the push is empty).
    pub baseline_after_push: PlayBaseline,
    /// Human-readable notes: a favourite conflict, an unrepresentable time.
    pub notes: Vec<String>,
}

impl RemotePlayData {
    /// Remote values in the same shape as the gamelist side.
    pub fn values(&self) -> PlayValues {
        PlayValues {
            favorite: self.favorite,
            playcount: self.play_count,
            gametime_minutes: self.play_time_ms / 60_000,
            lastplayed: self.last_played,
        }
    }
}

/// Reconcile local and remote play data against the last baseline.
pub fn merge_play(
    rom_id: i64,
    local: &PlayValues,
    remote: &RemotePlayData,
    base: &PlayBaseline,
    now: i64,
) -> PlayMerge {
    let remote_values = remote.values();

    // First sight of this game: take the union of both sides. Adding two
    // pre-existing histories would double-count plays recorded on both, so
    // counts and time are not pushed; a favourite the user set locally is
    // pushed, since that is cheap and idempotent.
    if !base.known {
        let merged = PlayValues {
            favorite: local.favorite || remote_values.favorite,
            playcount: local.playcount.max(remote_values.playcount),
            gametime_minutes: local.gametime_minutes.max(remote_values.gametime_minutes),
            lastplayed: later(local.lastplayed, remote_values.lastplayed),
        };
        let push = PlayPush {
            rom_id,
            favorite: (merged.favorite != remote_values.favorite).then_some(merged.favorite),
            sessions: Vec::new(),
        };
        return PlayMerge {
            baseline_after_push: PlayBaseline {
                synced_local: merged.clone(),
                synced_remote: PlayValues {
                    favorite: push.favorite.unwrap_or(remote_values.favorite),
                    ..remote_values
                },
                known: true,
            },
            merged,
            push,
            notes: Vec::new(),
        };
    }

    let mut notes = Vec::new();

    let local_favorite_changed = local.favorite != base.synced_local.favorite;
    let remote_favorite_changed = remote_values.favorite != base.synced_remote.favorite;

    let count_delta = local.playcount.saturating_sub(base.synced_local.playcount);
    let time_delta = local
        .gametime_minutes
        .saturating_sub(base.synced_local.gametime_minutes);
    let remote_count_delta = remote_values
        .playcount
        .saturating_sub(base.synced_remote.playcount);
    let remote_time_delta = remote_values
        .gametime_minutes
        .saturating_sub(base.synced_remote.gametime_minutes);

    if local_favorite_changed && remote_favorite_changed && local.favorite != remote_values.favorite
    {
        notes.push("favourite changed on both sides; kept the RetroBat value".to_string());
    }

    let merged = PlayValues {
        favorite: resolve_flag(
            local.favorite,
            local_favorite_changed,
            remote_values.favorite,
            remote_favorite_changed,
        ),
        // A local reset is not representable in RomM (there is no setter), so
        // the merged value never drops below the last reconciled local value.
        playcount: local.playcount.max(base.synced_local.playcount) + remote_count_delta,
        gametime_minutes: local
            .gametime_minutes
            .max(base.synced_local.gametime_minutes)
            + remote_time_delta,
        lastplayed: later(local.lastplayed, remote_values.lastplayed),
    };

    if count_delta == 0 && time_delta > 0 {
        notes.push(
            "play time advanced without a new launch; it reaches RomM with the next play"
                .to_string(),
        );
    }

    let sessions = sessions_for(rom_id, count_delta, time_delta, local.lastplayed, now);
    let push = PlayPush {
        rom_id,
        favorite: local_favorite_changed.then_some(local.favorite),
        sessions,
    };

    // RomM's state once the push lands: the remote values plus the local
    // changes. With no push this is just the current remote snapshot.
    let pushed_ends: Vec<i64> = push
        .sessions
        .iter()
        .filter_map(|session| parse_rfc3339(&session.end_time))
        .collect();
    let synced_remote = PlayValues {
        favorite: push.favorite.unwrap_or(remote_values.favorite),
        playcount: remote_values.playcount + push.sessions.len() as u64,
        gametime_minutes: remote_values.gametime_minutes + time_delta,
        lastplayed: pushed_ends
            .into_iter()
            .fold(remote_values.lastplayed, |acc, end| later(acc, Some(end))),
    };
    PlayMerge {
        baseline_after_push: PlayBaseline {
            synced_local: merged.clone(),
            synced_remote,
            known: true,
        },
        merged,
        push,
        notes,
    }
}

/// Only the side that changed decides; both changed → the local value wins.
fn resolve_flag(local: bool, local_changed: bool, remote: bool, remote_changed: bool) -> bool {
    if local_changed {
        local
    } else if remote_changed {
        remote
    } else {
        local
    }
}

fn later(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, None) => a,
        (None, b) => b,
    }
}

/// One session per local launch, sharing the elapsed time between them and
/// ending at the gamelist's last-played time (never in the future).
fn sessions_for(
    rom_id: i64,
    count: u64,
    time_minutes: u64,
    lastplayed: Option<i64>,
    now: i64,
) -> Vec<PlaySessionUpload> {
    if count == 0 {
        return Vec::new();
    }
    let end = lastplayed.unwrap_or(now).min(now);
    let total_secs = time_minutes.saturating_mul(60).max(count);
    let per_secs = (total_secs / count).max(1) as i64;
    (0..count)
        .map(|index| {
            let end = end - (count as i64 - 1 - index as i64) * per_secs;
            let start = end - per_secs;
            PlaySessionUpload {
                rom_id,
                start_time: format_rfc3339(start),
                end_time: format_rfc3339(end),
                duration_ms: per_secs as u64 * 1000,
            }
        })
        .collect()
}

/// The gamelist elements for reconciled play values. Absent means "unset".
pub fn render_play_fields(values: &PlayValues) -> Vec<(&'static str, String)> {
    let mut fields = Vec::new();
    if values.favorite {
        fields.push(("favorite", "true".to_string()));
    }
    if values.playcount > 0 {
        fields.push(("playcount", values.playcount.to_string()));
    }
    if let Some(seconds) = values.lastplayed {
        if let Some(text) = format_lastplayed(seconds) {
            fields.push(("lastplayed", text));
        }
    }
    if values.gametime_minutes > 0 {
        fields.push(("gametime", values.gametime_minutes.to_string()));
    }
    fields
}

/// Read the play values from a gamelist entry's captured frontend nodes.
pub fn parse_play_fields(nodes: &[String]) -> PlayValues {
    let mut values = PlayValues::default();
    for node in nodes {
        let Some(name) = node_name(node) else {
            continue;
        };
        let text = element_text(node).unwrap_or_default();
        match name.as_str() {
            "favorite" => values.favorite = text.eq_ignore_ascii_case("true"),
            "playcount" => values.playcount = text.trim().parse().unwrap_or(0),
            "gametime" => values.gametime_minutes = text.trim().parse().unwrap_or(0),
            "lastplayed" => values.lastplayed = parse_lastplayed(&text),
            _ => {}
        }
    }
    values
}

/// The element name of one raw gamelist node (`<favorite>true</favorite>`).
pub fn node_name(node: &str) -> Option<String> {
    let mut reader = quick_xml::Reader::from_str(node);
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(element))
            | Ok(quick_xml::events::Event::Empty(element)) => {
                return Some(String::from_utf8_lossy(element.name().as_ref()).into_owned())
            }
            Ok(quick_xml::events::Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

fn element_text(node: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Value {
        #[serde(rename = "$text", default)]
        text: String,
    }
    quick_xml::de::from_str::<Value>(node).ok().map(|v| v.text)
}

fn parse_lastplayed(text: &str) -> Option<i64> {
    let naive = chrono::NaiveDateTime::parse_from_str(text.trim(), LASTPLAYED_FORMAT).ok()?;
    Some(naive.and_utc().timestamp())
}

fn format_lastplayed(seconds: i64) -> Option<String> {
    Some(
        DateTime::from_timestamp(seconds, 0)?
            .format(LASTPLAYED_FORMAT)
            .to_string(),
    )
}

pub(crate) fn parse_rfc3339(text: &str) -> Option<i64> {
    Some(DateTime::parse_from_rfc3339(text.trim()).ok()?.timestamp())
}

pub(crate) fn format_rfc3339(seconds: i64) -> String {
    DateTime::from_timestamp(seconds, 0)
        .unwrap_or_else(|| DateTime::from_timestamp(0, 0).expect("epoch is valid"))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

/// Fold RomM's collections, play sessions and per-ROM user data into one map
/// keyed by RomM ROM id.
pub fn build_remote_play(
    rom_user: &HashMap<i64, crate::romm::RomUserDto>,
    favorite_rom_ids: &[i64],
    sessions: &[crate::romm::PlaySessionDto],
) -> HashMap<i64, RemotePlayData> {
    let mut remote: HashMap<i64, RemotePlayData> = HashMap::new();
    for (rom_id, user) in rom_user {
        let entry = remote.entry(*rom_id).or_default();
        entry.last_played = user.last_played.as_deref().and_then(parse_rfc3339);
    }
    for rom_id in favorite_rom_ids {
        remote.entry(*rom_id).or_default().favorite = true;
    }
    for session in sessions {
        let Some(rom_id) = session.rom_id else {
            continue;
        };
        remote.entry(rom_id).or_default().add_session(session);
    }
    remote
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(play_count: u64, last: Option<i64>) -> RemotePlayData {
        RemotePlayData {
            play_count,
            last_played: last,
            ..Default::default()
        }
    }

    /// A baseline that has already reconciled a game at the given counts.
    fn synced(playcount: u64) -> PlayBaseline {
        let mut base = PlayBaseline {
            known: true,
            ..Default::default()
        };
        base.synced_local.playcount = playcount;
        base.synced_remote.playcount = playcount;
        base
    }

    #[test]
    fn first_sync_adopts_the_union_and_pushes_only_the_favourite() {
        // Pre-existing counts on both sides must not be added together.
        let local = PlayValues {
            favorite: true,
            playcount: 3,
            gametime_minutes: 30,
            lastplayed: Some(1_600_000_000),
        };
        let merge = merge_play(
            1,
            &local,
            &remote(1, Some(1_700_000_000)),
            &PlayBaseline::default(),
            1_700_000_100,
        );
        assert_eq!(merge.merged.playcount, 3);
        assert_eq!(merge.merged.gametime_minutes, 30);
        assert_eq!(merge.merged.lastplayed, Some(1_700_000_000));
        assert!(merge.merged.favorite);
        // No history is pushed, but the locally-set favourite is.
        assert_eq!(merge.push.favorite, Some(true));
        assert!(merge.push.sessions.is_empty());
        assert!(merge.baseline_after_push.synced_remote.favorite);
        assert_eq!(merge.baseline_after_push.synced_remote.playcount, 1);
        assert!(merge.baseline_after_push.known);
    }

    #[test]
    fn local_only_count_advance_becomes_one_session_per_launch() {
        let base = synced(1);
        let local = PlayValues {
            playcount: 3,
            gametime_minutes: 30,
            lastplayed: Some(1_700_000_000),
            ..Default::default()
        };
        let merge = merge_play(
            1,
            &local,
            &remote(1, Some(1_600_000_000)),
            &base,
            1_700_000_100,
        );
        assert_eq!(merge.merged.playcount, 3);
        assert_eq!(merge.push.sessions.len(), 2);
        assert_eq!(merge.baseline_after_push.synced_remote.playcount, 3);
        assert_eq!(merge.baseline_after_push.synced_local.playcount, 3);
    }

    #[test]
    fn remote_only_count_advance_pulls_in_without_a_session() {
        let base = synced(0);
        let local = PlayValues::default();
        let merge = merge_play(1, &local, &remote(2, None), &base, 0);
        assert_eq!(merge.merged.playcount, 2);
        assert!(merge.push.is_empty());
        assert_eq!(merge.baseline_after_push.synced_local.playcount, 2);
    }

    #[test]
    fn both_sides_advance_and_neither_is_lost() {
        let local = PlayValues {
            playcount: 2,
            ..Default::default()
        };
        let base = synced(1);
        let merge = merge_play(1, &local, &remote(4, None), &base, 0);
        assert_eq!(merge.merged.playcount, 5);
        assert_eq!(merge.push.sessions.len(), 1);
        // Remote ends at the merged value once the push lands; no recount later.
        assert_eq!(merge.baseline_after_push.synced_remote.playcount, 5);
        let again = merge_play(
            1,
            &merge.merged,
            &RemotePlayData {
                play_count: 5,
                ..Default::default()
            },
            &merge.baseline_after_push,
            0,
        );
        assert!(again.push.is_empty());
        assert_eq!(again.merged.playcount, 5);
    }

    #[test]
    fn a_reset_count_does_not_go_backwards_or_push() {
        let base = synced(10);
        let local = PlayValues {
            playcount: 0,
            ..Default::default()
        };
        let merge = merge_play(1, &local, &remote(10, None), &base, 0);
        assert!(merge.push.sessions.is_empty());
        assert_eq!(merge.merged.playcount, 10);
    }

    #[test]
    fn favourite_conflict_keeps_the_local_value_and_is_logged() {
        // Local and remote both changed, to different values.
        let mut base = PlayBaseline {
            known: true,
            ..Default::default()
        };
        base.synced_local.favorite = true;
        base.synced_remote.favorite = false;
        let local = PlayValues {
            favorite: false,
            ..Default::default()
        };
        let remote = RemotePlayData {
            favorite: true,
            ..Default::default()
        };
        let merge = merge_play(1, &local, &remote, &base, 0);
        assert!(!merge.merged.favorite);
        assert_eq!(merge.push.favorite, Some(false));
        assert!(merge.notes.iter().any(|note| note.contains("both sides")));

        // Only the remote changed: it is pulled in.
        let base = PlayBaseline {
            known: true,
            ..Default::default()
        };
        let merge = merge_play(1, &PlayValues::default(), &remote, &base, 0);
        assert!(merge.merged.favorite);
        assert!(merge.push.is_empty());
    }

    #[test]
    fn last_played_takes_the_later_value() {
        let mut base = synced(0);
        base.synced_local.lastplayed = Some(100);
        base.synced_remote.lastplayed = Some(100);
        let local = PlayValues {
            lastplayed: Some(200),
            ..Default::default()
        };
        let merge = merge_play(1, &local, &remote(0, Some(150)), &base, 300);
        assert_eq!(merge.merged.lastplayed, Some(200));
    }

    #[test]
    fn sessions_never_end_in_the_future() {
        let local = PlayValues {
            playcount: 1,
            lastplayed: Some(9_999_999_999),
            ..Default::default()
        };
        let merge = merge_play(
            1,
            &local,
            &RemotePlayData::default(),
            &synced(0),
            1_700_000_000,
        );
        let session = &merge.push.sessions[0];
        assert!(parse_rfc3339(&session.end_time).unwrap() <= 1_700_000_000);
        assert!(
            parse_rfc3339(&session.start_time).unwrap() < parse_rfc3339(&session.end_time).unwrap()
        );
    }

    #[test]
    fn rendered_fields_match_the_frontend_shape() {
        let values = PlayValues {
            favorite: true,
            playcount: 12,
            gametime_minutes: 90,
            lastplayed: Some(1_700_000_000),
        };
        let fields = render_play_fields(&values);
        assert_eq!(fields[0], ("favorite", "true".to_string()));
        assert_eq!(fields[1], ("playcount", "12".to_string()));
        assert_eq!(fields[3], ("gametime", "90".to_string()));
        let node = |name: &str, text: &str| format!("<{name}>{text}</{name}>");
        let parsed = parse_play_fields(&[
            node("favorite", "true"),
            node("playcount", "12"),
            node("lastplayed", "20231114T221320"),
            node("gametime", "90"),
        ]);
        assert!(parsed.favorite);
        assert_eq!(parsed.playcount, 12);
        assert_eq!(parsed.gametime_minutes, 90);
        assert_eq!(parsed.lastplayed, Some(1_700_000_000));
    }
}
