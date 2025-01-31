#![allow(clippy::blocks_in_conditions)]

use crate::error::{RedirectTo, Result};
use askama::Template;
use chrono::{DateTime, TimeZone, Utc};
use rocket::form::Form;
use rocket::http::uri::Absolute;
use rocket::http::{self, CookieJar};
use rocket::response::Redirect;
use rocket::{get, post, FromForm};
use uuid::Uuid;

use crate::db::{self, Author, NewRoom, Room};
use crate::{Context, TplContext};
use rocket::State;

use super::auth::LoggedInSession;

#[derive(Template)]
#[template(path = "room_manager/create_room.html")]
struct EditRoom<'a> {
    base: TplContext<'a>,
    room: Option<Room>,
}

#[derive(FromForm, Debug)]
struct CreateRoomForm<'a> {
    room_name: &'a str,
    room_description: &'a str,
    close_date: &'a str,
    tz_offset: i32,
    room_url: &'a str,
}

#[derive(Template)]
#[template(path = "room_manager/rooms.html")]
struct ListRoomsTpl<'a> {
    base: TplContext<'a>,
    open_rooms: Vec<Room>,
    closed_rooms: Vec<Room>,
}

#[get("/rooms")]
fn my_rooms<'a>(
    ctx: &State<Context>,
    session: LoggedInSession,
    cookies: &CookieJar,
) -> Result<ListRoomsTpl<'a>> {
    let author_filter = if session.0.is_admin {
        Author::Any
    } else {
        Author::User(session.user_id())
    };

    Ok(ListRoomsTpl {
        base: TplContext::from_session("rooms", session.0, cookies),
        open_rooms: db::list_rooms(db::RoomStatus::Open, author_filter, 50, ctx)?,
        closed_rooms: db::list_rooms(db::RoomStatus::Closed, author_filter, 50, ctx)?,
    })
}
#[get("/create-room")]
fn create_room<'a>(session: LoggedInSession, cookies: &CookieJar) -> Result<EditRoom<'a>> {
    Ok(EditRoom {
        base: TplContext::from_session("create-room", session.0, cookies),
        room: None,
    })
}

fn parse_date(date: &str, tz_offset: i32) -> Result<DateTime<Utc>> {
    let offset = chrono::FixedOffset::west_opt(tz_offset * 60)
        .ok_or_else(|| crate::error::Error(anyhow::anyhow!("Wrong timezone offset")))?;
    let datetime = chrono::NaiveDateTime::parse_from_str(date, "%Y-%m-%dT%H:%M")?;
    let date = offset
        .from_local_datetime(&datetime)
        .single()
        .ok_or_else(|| crate::error::Error(anyhow::anyhow!("Cannot parse passed datetime")))?;

    Ok(date.into())
}

#[post("/create-room", data = "<room_form>")]
fn create_room_submit(
    redirect_to: &RedirectTo,
    ctx: &State<Context>,
    room_form: Form<CreateRoomForm>,
    session: LoggedInSession,
) -> Result<Redirect> {
    redirect_to.set("/create-room");

    if room_form.room_name.is_empty() {
        return Err(anyhow::anyhow!("The room name shouldn't be empty").into());
    }

    let author_id = session.user_id();
    let close_date = parse_date(room_form.close_date, room_form.tz_offset)?;
    let new_room = db::create_room(
        room_form.room_name,
        &room_form.room_description.trim(),
        &close_date,
        author_id,
        ctx,
    )?;

    Ok(Redirect::to(format!("/room/{}", new_room.id)))
}

#[get("/edit-room/<room_id>")]
fn edit_room<'a>(
    ctx: &State<Context>,
    room_id: Uuid,
    session: LoggedInSession,
    cookies: &CookieJar,
) -> Result<EditRoom<'a>> {
    let room = crate::db::get_room(room_id, ctx)?;
    let is_my_room = session.0.is_admin || session.0.user_id == Some(room.author_id);

    if !is_my_room {
        return Err(anyhow::anyhow!("You're not allowed to edit this room").into());
    }

    Ok(EditRoom {
        base: TplContext::from_session("room", session.0, cookies),
        room: Some(room),
    })
}

#[post("/edit-room/<room_id>", data = "<room_form>")]
fn edit_room_submit(
    redirect_to: &RedirectTo,
    room_id: Uuid,
    room_form: Form<CreateRoomForm>,
    ctx: &State<Context>,
    session: LoggedInSession,
) -> Result<Redirect> {
    redirect_to.set(&format!("/edit-room/{}", room_id));

    let room = crate::db::get_room(room_id, ctx)?;
    let is_my_room = session.0.is_admin || session.0.user_id == Some(room.author_id);
    if !is_my_room {
        return Err(anyhow::anyhow!("You're not allowed to edit this room").into());
    }

    let room_url = room_form.room_url.trim();
    if !room_url.is_empty() {
        if let Err(e) = http::uri::Uri::parse::<Absolute>(room_url) {
            return Err(anyhow::anyhow!("Error while parsing room URL: {}", e).into());
        }
    }

    let new_room = NewRoom {
        id: crate::diesel_uuid::Uuid(room_id),
        name: room_form.room_name,
        description: &room_form.room_description.trim(),
        close_date: parse_date(room_form.close_date, room_form.tz_offset)?.naive_utc(),
        room_url,
        author_id: None, // (Skips updating that field)
    };
    crate::db::update_room(&new_room, ctx)?;

    Ok(Redirect::to(format!("/room/{}", room_id)))
}

pub fn routes() -> Vec<rocket::Route> {
    rocket::routes![
        create_room,
        my_rooms,
        create_room_submit,
        edit_room,
        edit_room_submit
    ]
}
