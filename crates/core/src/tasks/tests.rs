//! The whole path, against one mock server that plays Google and the AI.

use std::time::Duration;

use jiff::Zoned;
use jiff::civil::{date, time};
use serde_json::{Value, json};

use super::google::{CALENDAR_SCOPE, Credentials, Endpoints, TASKS_SCOPE, TokenSource};
use super::*;
use crate::ai::mock::{Mock, Reply, reply, serve};
use crate::ai::{AiProvider, Effort, Kind};

fn said() -> Said {
    let z: Zoned = "2026-11-06T18:40[Europe/Berlin]".parse().unwrap();
    Said::at(z.timestamp().as_millisecond(), "Europe/Berlin")
}

fn book() -> (Book, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("mirza-book-test-{}", store::new_id(0)));
    (Book::new(Store::new(&dir)), dir)
}

fn setup(m: &Mock, with_ai: bool) -> Setup {
    let source = TokenSource::Refresh(Credentials {
        client_id: "cid".into(),
        client_secret: Some("sec".into()),
        refresh_token: "rt".into(),
    });
    let google = Google::new(source, None, Endpoints::at(&m.base), Duration::from_millis(800)).unwrap();
    let ai = with_ai.then(|| {
        let p = AiProvider { model: "m".into(), base_url: format!("{}/v1", m.base), effort: Effort::Off };
        crate::ai::Client::new(Kind::Local, &p, None, None, Duration::from_secs(5)).unwrap()
    });
    Setup {
        settings: TaskSettings::default(),
        ai,
        ai_problem: "no AI is chosen for voice tasks".into(),
        terms: vec![],
        google: Some(google),
        google_problem: String::new(),
    }
}

fn token(scope: &str) -> Reply {
    reply(200, json!({"access_token": "at", "expires_in": 3599, "scope": scope, "token_type": "Bearer"}).to_string())
}

fn both() -> Reply {
    token(&format!("{TASKS_SCOPE} {CALENDAR_SCOPE}"))
}

fn lists() -> Reply {
    reply(200, json!({"items": [{"id": "L1", "title": "Personal"}, {"id": "L2", "title": "Acme"}]}).to_string())
}

/// The AI's answer, as a local server sends it.
fn ai(fields: Value) -> Reply {
    let mut a = json!({"title": "Call Sara", "note": "about the contract", "list": "Acme", "date": "2026-11-07",
        "date_calendar": "gregorian", "time": "15:00", "minutes": 0, "guessed": ""});
    for (k, v) in fields.as_object().unwrap() {
        a[k] = v.clone();
    }
    let body =
        json!({"choices": [{"message": {"role": "assistant", "content": a.to_string()}, "finish_reason": "stop"}]});
    reply(200, body.to_string())
}

fn task(id: &str) -> Reply {
    reply(200, json!({"id": id, "title": "Call Sara"}).to_string())
}

fn ok() -> Reply {
    reply(200, "{}")
}

fn calls(m: &Mock) -> Vec<String> {
    m.seen().iter().map(|s| format!("{} {}", s.method, s.target.split('?').next().unwrap_or_default())).collect()
}

fn saved(o: Outcome) -> Saved {
    match o {
        Outcome::Saved(s) => s,
        o => panic!("not saved: {o:?}"),
    }
}

const SAY: &str = "call Sara tomorrow at 3 about the contract, it's for Acme";

#[tokio::test]
async fn a_timed_task_becomes_a_task_and_an_event() {
    let m = serve(vec![both(), lists(), ai(json!({})), task("T1"), ok()]).await;
    let s = setup(&m, true);
    let (b, dir) = book();
    let e = b.capture(SAY, said(), false).unwrap();
    assert_eq!(b.waiting().len(), 1, "kept before anything is sent");

    let out = saved(b.process(&s, &e.id).await);
    assert_eq!(out.draft.title, "Call Sara");
    assert_eq!(out.task, store::TaskRef { list: "L2".into(), id: "T1".into() });
    assert_eq!(out.event.as_deref(), Some(store::event_id(&e.id, 0).as_str()));
    assert!(b.waiting().is_empty());
    assert_eq!(b.recent()[0], out);

    assert_eq!(
        calls(&m),
        [
            "POST /token",
            "GET /tasks/v1/users/@me/lists",
            "POST /v1/chat/completions",
            "POST /tasks/v1/lists/L2/tasks",
            "POST /calendar/v3/calendars/primary/events",
        ]
    );
    let seen = m.seen();
    assert_eq!(
        seen[3].body,
        json!({"title": "Call Sara", "notes": "about the contract", "due": "2026-11-07T00:00:00.000Z"})
    );
    let ev = &seen[4].body;
    assert_eq!(ev["id"], json!(store::event_id(&e.id, 0)));
    assert_eq!(ev["start"], json!({"dateTime": "2026-11-07T15:00:00+01:00", "timeZone": "Europe/Berlin"}));
    assert_eq!(ev["end"]["dateTime"], json!("2026-11-07T15:30:00+01:00"));
    assert_eq!(ev["reminders"], json!({"useDefault": true}));
    assert_eq!(ev["extendedProperties"]["private"]["mirza"], json!(e.id));

    let (title, body) = notification(&Outcome::Saved(out), &[], date(2026, 11, 6)).unwrap();
    assert_eq!(title, "Call Sara");
    assert_eq!(body, "a list · tomorrow 15:00 · in Calendar too");
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn when_the_ai_fails_the_words_are_saved_as_said() {
    let refused = reply(400, r#"{"error":{"message":"model not loaded"}}"#);
    let m = serve(vec![both(), lists(), refused, task("T1")]).await;
    let s = setup(&m, true);
    let (b, dir) = book();
    let e = b.capture(SAY, said(), false).unwrap();
    let out = saved(b.process(&s, &e.id).await);
    assert!(out.draft.raw);
    assert_eq!(out.draft.title, "call Sara tomorrow at 3…");
    assert_eq!(out.draft.note, SAY);
    assert_eq!(out.draft.date, None);
    assert!(out.draft.guesses[0].starts_with("saved as said: "), "{:?}", out.draft.guesses);
    assert_eq!(out.event, None);
    assert_eq!(calls(&m)[3], "POST /tasks/v1/lists/@default/tasks");
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn without_ai_the_words_are_saved_as_said() {
    let m = serve(vec![both(), lists(), task("T1")]).await;
    let s = setup(&m, false);
    let (b, dir) = book();
    let e = b.capture("buy milk", said(), false).unwrap();
    let out = saved(b.process(&s, &e.id).await);
    assert_eq!((out.draft.title.as_str(), out.draft.note.as_str()), ("buy milk", ""));
    assert_eq!(out.draft.guesses, ["saved as said: no AI is chosen for voice tasks"]);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn a_preview_saves_nothing() {
    let m = serve(vec![both(), lists(), ai(json!({"date": "", "time": ""}))]).await;
    let s = setup(&m, true);
    let (b, dir) = book();
    let d = b.preview(&s, SAY).await.unwrap();
    assert_eq!((d.title.as_str(), d.list.as_str(), d.date), ("Call Sara", "L2", None));
    assert!(b.waiting().is_empty() && b.recent().is_empty());
    assert_eq!(calls(&m), ["POST /token", "GET /tasks/v1/users/@me/lists", "POST /v1/chat/completions"]);
    let err = b.preview(&setup(&m, false), SAY).await.unwrap_err();
    assert_eq!(err, "no AI is chosen for voice tasks");
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn offline_tasks_wait_and_are_saved_later() {
    let down = reply(503, r#"{"error":{"code":503,"message":"Backend Error"}}"#);
    let m = serve(vec![both(), down, lists(), ai(json!({"time": ""})), task("T1")]).await;
    let s = setup(&m, true);
    let (b, dir) = book();
    let e = b.capture(SAY, said(), false).unwrap();

    let Outcome::Waiting { entry, reason } = b.process(&s, &e.id).await else { panic!("not waiting") };
    assert_eq!(entry.attempts, 1);
    assert!(entry.next_try_ms > jiff::Timestamp::now().as_millisecond() + 20_000);
    assert!(reason.contains("Google"), "{reason}");
    let (_, body) = notification(&Outcome::Waiting { entry: entry.clone(), reason }, &[], date(2026, 11, 6)).unwrap();
    assert!(body.ends_with("Mirza tries again by itself."), "{body}");
    // Not due yet, so a tick does nothing.
    assert!(b.process_due(&s).await.is_empty());
    assert_eq!(b.waiting().len(), 1);

    b.retry_now();
    let out = b.process_due(&s).await;
    assert_eq!(out.len(), 1);
    let out = saved(out.into_iter().next().unwrap());
    assert_eq!(out.event, None, "no time was said");
    assert!(b.waiting().is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn a_task_whose_answer_was_lost_is_not_sent_twice() {
    let slow = Reply { status: 200, body: json!({"id": "T1"}).to_string(), delay: Duration::from_millis(1500) };
    let found = reply(
        200,
        json!({"items": [{"id": "T0", "title": "Other"}, {"id": "T1", "title": "Call Sara", "notes": "about the contract"}]})
            .to_string(),
    );
    let m = serve(vec![both(), lists(), ai(json!({})), slow, found, ok()]).await;
    let s = setup(&m, true);
    let (b, dir) = book();
    let e = b.capture(SAY, said(), false).unwrap();

    let Outcome::Waiting { entry, .. } = b.process(&s, &e.id).await else { panic!("not waiting") };
    assert!(entry.progress.task_sent_ms > 0 && entry.progress.task.is_none());
    assert_eq!(entry.draft.as_ref().unwrap().title, "Call Sara", "the AI isn't asked again");

    let out = saved(b.process(&s, &e.id).await);
    assert_eq!(out.task.id, "T1");
    let c = calls(&m);
    assert_eq!(c.iter().filter(|c| c.as_str() == "POST /tasks/v1/lists/L2/tasks").count(), 1, "{c:?}");
    assert_eq!(c[4], "GET /tasks/v1/lists/L2/tasks");
    assert_eq!(c.len(), 6, "lists came from the last read: {c:?}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn signed_out_tasks_are_kept() {
    let m = serve(vec![]).await;
    let mut s = setup(&m, true);
    s.google = None;
    s.google_problem = NOT_SIGNED_IN.into();
    let (b, dir) = book();
    let e = b.capture(SAY, said(), false).unwrap();
    let Outcome::Stuck { entry, reason } = b.process(&s, &e.id).await else { panic!("not stuck") };
    assert_eq!(reason, NOT_SIGNED_IN);
    assert_eq!(b.waiting(), std::slice::from_ref(&entry));

    // Editing it before it's saved changes what will be sent.
    let edit = Edit {
        title: "Call  Sara back".into(),
        note: String::new(),
        list: "L1".into(),
        date: Some(date(2026, 11, 9)),
        time: None,
        minutes: 0,
    };
    let Ok(Outcome::Stuck { entry, .. }) = b.edit(&s, &e.id, edit).await else { panic!("not stuck") };
    let d = entry.draft.unwrap();
    assert_eq!((d.title.as_str(), d.list.as_str(), d.date), ("Call Sara back", "L1", Some(date(2026, 11, 9))));
    assert!(m.seen().is_empty());

    b.undo(&s, &e.id).await.unwrap();
    assert!(b.waiting().is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn without_calendar_access_a_timed_task_is_a_task_only() {
    let m = serve(vec![token(TASKS_SCOPE), lists(), ai(json!({})), task("T1")]).await;
    let mut s = setup(&m, true);
    s.settings.timed_due_date = false;
    let (b, dir) = book();
    let e = b.capture(SAY, said(), false).unwrap();
    let out = saved(b.process(&s, &e.id).await);
    assert_eq!(out.event, None);
    assert!(out.draft.guesses.iter().any(|g| g.contains("doesn't include Calendar")), "{:?}", out.draft.guesses);
    assert_eq!(m.seen()[3].body["due"], json!("2026-11-07T00:00:00.000Z"), "it keeps its day");
    assert_eq!(m.seen().len(), 4);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn timed_tasks_can_go_without_a_due_date() {
    let m = serve(vec![both(), lists(), ai(json!({"minutes": 90})), task("T1"), ok()]).await;
    let mut s = setup(&m, true);
    s.settings.timed_due_date = false;
    let (b, dir) = book();
    let e = b.capture(SAY, said(), false).unwrap();
    saved(b.process(&s, &e.id).await);
    let seen = m.seen();
    assert_eq!(seen[3].body["due"], Value::Null);
    assert_eq!(seen[4].body["end"]["dateTime"], json!("2026-11-07T16:30:00+01:00"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn undo_deletes_the_task_and_the_event() {
    let gone = reply(404, r#"{"error":{"code":404,"message":"Not Found"}}"#);
    let m = serve(vec![both(), lists(), ai(json!({})), task("T1"), ok(), reply(204, ""), gone]).await;
    let s = setup(&m, true);
    let (b, dir) = book();
    let e = b.capture(SAY, said(), false).unwrap();
    saved(b.process(&s, &e.id).await);
    b.undo(&s, &e.id).await.unwrap();
    assert!(b.recent().is_empty());
    let c = calls(&m);
    assert_eq!(
        c[5..],
        [
            format!("DELETE /calendar/v3/calendars/primary/events/{}", store::event_id(&e.id, 0)),
            "DELETE /tasks/v1/lists/L2/tasks/T1".to_owned(),
        ]
    );
    let again = b.undo(&s, &e.id).await.unwrap_err();
    assert!(again.contains("isn't among Mirza's recent tasks"), "{again}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn edits_reach_google() {
    let moved = reply(200, json!({"id": "T9"}).to_string());
    let m = serve(vec![
        both(),
        lists(),
        ai(json!({})),
        task("T1"),
        ok(),
        // 1: another time and list: move, patch the task, patch the event.
        moved,
        ok(),
        ok(),
        // 2: no time: patch the task, delete the event.
        ok(),
        reply(204, ""),
        // 3: a time again: patch the task, a fresh event.
        ok(),
        ok(),
    ])
    .await;
    let s = setup(&m, true);
    let (b, dir) = book();
    let e = b.capture(SAY, said(), false).unwrap();
    saved(b.process(&s, &e.id).await);
    let first = store::event_id(&e.id, 0);

    let mut edit = Edit {
        title: "Call Sara".into(),
        note: "about the contract".into(),
        list: "L1".into(),
        date: Some(date(2026, 11, 7)),
        time: Some(time(16, 0, 0, 0)),
        minutes: 0,
    };
    let s1 = saved(b.edit(&s, &e.id, edit.clone()).await.unwrap());
    assert_eq!(s1.task, store::TaskRef { list: "L1".into(), id: "T9".into() });
    assert_eq!(s1.event.as_deref(), Some(first.as_str()));

    edit.time = None;
    let s2 = saved(b.edit(&s, &e.id, edit.clone()).await.unwrap());
    assert_eq!((s2.event.as_ref(), s2.event_gen), (None, 1));

    edit.time = Some(time(9, 0, 0, 0));
    let s3 = saved(b.edit(&s, &e.id, edit.clone()).await.unwrap());
    assert_eq!(s3.event, Some(store::event_id(&e.id, 1)));
    assert_eq!(b.recent()[0], s3);

    let c = calls(&m);
    assert_eq!(
        c[5..],
        [
            "POST /tasks/v1/lists/L2/tasks/T1/move".to_owned(),
            "PATCH /tasks/v1/lists/L1/tasks/T9".to_owned(),
            format!("PATCH /calendar/v3/calendars/primary/events/{first}"),
            "PATCH /tasks/v1/lists/L1/tasks/T9".to_owned(),
            format!("DELETE /calendar/v3/calendars/primary/events/{first}"),
            "PATCH /tasks/v1/lists/L1/tasks/T9".to_owned(),
            "POST /calendar/v3/calendars/primary/events".to_owned(),
        ]
    );
    let seen = m.seen();
    assert!(seen[5].target.ends_with("destinationTasklist=L1"), "{}", seen[5].target);
    assert_eq!(seen[7].body["start"]["dateTime"], json!("2026-11-07T16:00:00+01:00"));
    assert_eq!(seen[11].body["id"], json!(store::event_id(&e.id, 1)));

    let bad = Edit { title: " ".into(), ..edit.clone() };
    assert_eq!(b.edit(&s, &e.id, bad).await.unwrap_err(), "A task needs a title.");
    let bad = Edit { date: None, ..edit };
    assert_eq!(b.edit(&s, &e.id, bad).await.unwrap_err(), "A time needs a day.");
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn lists_come_from_the_last_read_while_fresh() {
    let m = serve(vec![both(), lists(), lists()]).await;
    let s = setup(&m, false);
    let (b, dir) = book();
    assert_eq!(b.lists(&s, false).await.unwrap().len(), 2);
    assert_eq!(b.lists(&s, false).await.unwrap().len(), 2);
    assert_eq!(m.seen().len(), 2, "the second came from disk");
    assert_eq!(b.lists(&s, true).await.unwrap().len(), 2);
    assert_eq!(m.seen().len(), 3);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn backoff_grows_and_stops_at_15_minutes() {
    let s: Vec<i64> = (1..=8).map(|a| backoff_ms(a) / 1000).collect();
    assert_eq!(s, [30, 60, 120, 300, 600, 900, 900, 900]);
}
