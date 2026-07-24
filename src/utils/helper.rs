use nanoid::nanoid;

use crate::state::{ AppState };

const ALPHABET: &[char] = &[
    'a',
    'b',
    'c',
    'd',
    'e',
    'f',
    'g',
    'h',
    'i',
    'j',
    'k',
    'l',
    'm',
    'n',
    'o',
    'p',
    'q',
    'r',
    's',
    't',
    'u',
    'v',
    'w',
    'x',
    'y',
    'z',
];

pub fn generate_room_id() -> String {
    let part1 = nanoid!(4, ALPHABET);
    let part2 = nanoid!(4, ALPHABET);
    let part3 = nanoid!(4, ALPHABET);

    format!("{}-{}-{}", part1, part2, part3)
}

pub async fn subscribe_existing_tracks(state: &AppState, room_id: &str, subscriber_id: &str) {
    let existing_tracks = {
        let rooms = state.rooms.read().await;

        let room = match rooms.get(room_id) {
            Some(r) => r,
            None => {
                return;
            }
        };

        room.published_tracks
            .iter()
            .filter(|(publisher_id, _)| *publisher_id != subscriber_id)
            .map(|(publisher_id, tracks)| { (publisher_id.clone(), tracks.clone()) })
            .collect::<Vec<_>>()
    };

    for (publisher_id, tracks) in existing_tracks {
        for (source, track) in tracks {
            let subscriber_pc = {
                let rooms = state.rooms.read().await;

                match rooms.get(room_id).and_then(|room| room.server_peers.get(subscriber_id)) {
                    Some(peer) => peer.subscriber_pc.clone(),
                    None => {
                        continue;
                    }
                }
            };

            if
                let Err(err) = state.track_repository.add_forwarder(
                    state,
                    room_id,
                    &publisher_id,
                    subscriber_id,
                    subscriber_pc,
                    source,
                    track
                ).await
            {
                log::error!(
                    "Failed forwarding existing track {} -> {}: {:?}",
                    publisher_id,
                    subscriber_id,
                    err
                );
            }
        }
    }
}
