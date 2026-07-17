use crate::{ socket::handlers::room_feed::build_room_presence, state::AppState };

pub async fn broadcast_room_presence(state: &AppState, room_id: &str) {
    let watchers = {
        let watchers = state.watchers.read().await;
        println!(
            "WATCHERS FOR ROOM {} = {}",
            room_id,
            watchers
                .get(room_id)
                .map(|w| w.len())
                .unwrap_or(0)
        );

        let Some(room_watchers) = watchers.get(room_id) else {
            return;
        };

        room_watchers
            .iter()
            .map(|(user_id, sender)| (user_id.clone(), sender.clone()))
            .collect::<Vec<_>>()
    };

    println!("Broadcasting presence to {} watchers", watchers.len());

    for (user_id, watcher) in watchers {
        if let Some(payload) = build_room_presence(state, room_id, &user_id).await {
            let _ = watcher.send(payload);
        }
    }
}
