use std::collections::BTreeSet;

use serde::Deserialize;

pub type PlayerId = String;

#[derive(Debug, Clone, Deserialize)]
pub struct Player {
    #[serde(rename = "playerId")]
    pub player_id: PlayerId,
    pub name: String,
    pub level: i32,
}

#[derive(Debug, Default)]
pub struct WorldState {
    pub version: String,
    pub worldguid: String,
    pub players: BTreeSet<PlayerId>,
}

#[derive(Debug, Default)]
pub struct PlayerDiff {
    pub joined: Vec<PlayerId>,
    pub left: Vec<PlayerId>,
}

impl WorldState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn observe(&mut self, players: &[Player]) -> PlayerDiff {
        let current: BTreeSet<PlayerId> = players.iter().map(|p| p.player_id.clone()).collect();
        let joined: Vec<PlayerId> = current.difference(&self.players).cloned().collect();
        let left: Vec<PlayerId> = self.players.difference(&current).cloned().collect();
        self.players = current;
        PlayerDiff { joined, left }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn p(id: &str) -> Player {
        Player {
            player_id: id.into(),
            name: id.into(),
            level: 1,
        }
    }

    #[test]
    fn empty_to_two_joins() {
        let mut ws = WorldState::new();
        let diff = ws.observe(&[p("a"), p("b")]);
        assert_eq!(diff.joined, vec!["a", "b"]);
        assert!(diff.left.is_empty());
        assert_eq!(ws.players.len(), 2);
    }

    #[test]
    fn leave_produces_left() {
        let mut ws = WorldState::new();
        ws.observe(&[p("a"), p("b")]);
        let diff = ws.observe(&[p("a")]);
        assert_eq!(diff.left, vec!["b"]);
        assert!(diff.joined.is_empty());
    }

    #[test]
    fn replace_one_with_another() {
        let mut ws = WorldState::new();
        ws.observe(&[p("a")]);
        let diff = ws.observe(&[p("b")]);
        assert_eq!(diff.joined, vec!["b"]);
        assert_eq!(diff.left, vec!["a"]);
    }

    #[test]
    fn idempotent_observe() {
        let mut ws = WorldState::new();
        ws.observe(&[p("a"), p("b")]);
        let diff = ws.observe(&[p("a"), p("b")]);
        assert!(diff.joined.is_empty() && diff.left.is_empty());
    }
}
