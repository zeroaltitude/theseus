//! The guild file's reread keeps the forums the bot made (theseus-p5g63),
//! and a forum post's first message keeps its own id beside other traffic
//! (theseus-o366s).

use super::*;

/// A forum post's first message has the post's id, however many other
/// messages are being written at the same moment (theseus-o366s).
///
/// The fake answers every connection on its own thread. The post's first
/// message used to be pushed under one hold of the state lock and then given
/// the post's id under a second, through `messages.last_mut()`, so a message
/// another connection pushed in between took the post's id and its V2 flag.
/// After that the post's own first message had an id no `PATCH
/// /channels/<post>/messages/<post>` could address: every card edit answered
/// 404 Unknown Message for the rest of the process, and a test waiting for
/// the post's last state waited out its whole 100 s cap.
#[test]
fn a_forum_posts_first_message_keeps_its_id_beside_other_writes() {
    let fake = FakeDiscord::start();
    fake.set_guild(guild());
    fake.set_forum(LAB);
    let addr = fake.addr.clone();
    let call = |m: &str, p: &str, b: &Value| http(&addr, m, &format!("/api/v10{p}"), b);

    // Other traffic, all the while: messages into another channel. The hole
    // this guards is two lock holds wide, so the traffic is heavy and the
    // posts are many; one hit is enough and 150 posts beside six writers
    // find one all but always.
    let noise: Vec<_> = (0..6)
        .map(|w| {
            let addr = fake.addr.clone();
            std::thread::spawn(move || {
                for i in 0..300 {
                    http(
                        &addr,
                        "POST",
                        &format!("/api/v10/channels/{COMMONS}/messages"),
                        &json!({"content": format!("noise {w}.{i}")}),
                    );
                }
            })
        })
        .collect();

    let mut posts = Vec::new();
    for i in 0..150 {
        let body = json!({"name": format!("post {i}"), "auto_archive_duration": 1440,
                          "message": {"content": format!("card {i}")}});
        let (status, made) = call("POST", &format!("/channels/{LAB}/threads"), &body);
        assert_eq!(status, 200, "{made}");
        posts.push(made["id"].as_str().unwrap().to_string());
    }
    noise.into_iter().for_each(|w| w.join().unwrap());

    // Each post's first message is addressable at the post's own id, which
    // is what an edit of its card uses.
    for (i, post) in posts.iter().enumerate() {
        let (status, got) = call(
            "PATCH",
            &format!("/channels/{post}/messages/{post}"),
            &json!({"content": format!("edited {i}")}),
        );
        assert_eq!(status, 200, "post {post} has no first message: {got}");
    }
    // And no other message was given a post's id along the way.
    for post in &posts {
        let held: Vec<String> = fake
            .messages(post.parse().unwrap())
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(held, vec![post.clone()], "post {post} holds just its own");
    }
}

/// A guild watched from a file is read again at every request, and the
/// forum the bot made in it stays (theseus-p5g63): it was dropped, so a
/// binding that recorded it found it gone, made a second, and read tags
/// from a channel the stand-in did not know.
#[test]
fn a_forum_the_bot_made_survives_the_guild_files_reread() {
    let fake = FakeDiscord::start();
    let d = tempfile::tempdir().unwrap();
    let file = d.path().join("guild.json");
    std::fs::write(&file, serde_json::to_string(&guild()).unwrap()).unwrap();
    fake.watch_guild_file(&file).unwrap();
    let call = |m: &str, p: &str, b: &Value| http(&fake.addr, m, &format!("/api/v10{p}"), b);
    let body = json!({"name": "theseus", "type": 15, "available_tags": [{"name": "done"}]});
    let made = call("POST", &format!("/guilds/{DEFAULT_GUILD}/channels"), &body).1;
    let id = made["id"].as_str().unwrap().to_string();
    let (status, forum) = call("GET", &format!("/channels/{id}"), &Value::Null);
    assert_eq!((status, forum["type"].as_u64()), (200, Some(15)), "{forum}");
    assert_eq!(forum["available_tags"].as_array().map(Vec::len), Some(1));
    let listed = call(
        "GET",
        &format!("/guilds/{DEFAULT_GUILD}/channels"),
        &Value::Null,
    )
    .1;
    assert!(
        listed.as_array().unwrap().iter().any(|c| c["id"] == id),
        "{listed}"
    );
}
