use serde::{ Deserialize, Serialize };

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum SignalMessage {
    JOIN {
        room_id: String,
        user_id: String,
        sender_name: Option<String>,
    },

    JoinedAck {
        room_id: String,
        user_id: String,
    },

    JoinFailed {
        reason: String,
    },

    OFFER {
        target: String,
        sdp: String,
        room_id: String,
        user_id: String,
    },

    ANSWER {
        target: String,
        sdp: String,
        room_id: String,
        user_id: String,
    },

    ICE {
        target: String,
        candidate: String,
        room_id: String,
        user_id: String,
    },

    UserJoined {
        user_id: String,
        name: String,
    },

    UserLeft {
        user_id: String,
    },
}
