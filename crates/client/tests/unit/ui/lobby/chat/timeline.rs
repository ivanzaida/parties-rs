use chrono::Weekday;

use super::{format_chat_time, local_chat_date, month_key, weekday_key};

#[test]
fn format_chat_time_treats_seconds_and_milliseconds_as_same_instant() {
  assert_eq!(format_chat_time(1_700_000_000), format_chat_time(1_700_000_000_000));
}

#[test]
fn local_chat_date_treats_seconds_and_milliseconds_as_same_instant() {
  assert_eq!(local_chat_date(1_700_000_000), local_chat_date(1_700_000_000_000));
}

#[test]
fn weekday_keys_cover_every_weekday() {
  assert_eq!(weekday_key(Weekday::Mon), "date.weekday.monday");
  assert_eq!(weekday_key(Weekday::Tue), "date.weekday.tuesday");
  assert_eq!(weekday_key(Weekday::Wed), "date.weekday.wednesday");
  assert_eq!(weekday_key(Weekday::Thu), "date.weekday.thursday");
  assert_eq!(weekday_key(Weekday::Fri), "date.weekday.friday");
  assert_eq!(weekday_key(Weekday::Sat), "date.weekday.saturday");
  assert_eq!(weekday_key(Weekday::Sun), "date.weekday.sunday");
}

#[test]
fn month_keys_cover_calendar_months_and_default_to_january() {
  assert_eq!(month_key(1), "date.month.january");
  assert_eq!(month_key(2), "date.month.february");
  assert_eq!(month_key(3), "date.month.march");
  assert_eq!(month_key(4), "date.month.april");
  assert_eq!(month_key(5), "date.month.may");
  assert_eq!(month_key(6), "date.month.june");
  assert_eq!(month_key(7), "date.month.july");
  assert_eq!(month_key(8), "date.month.august");
  assert_eq!(month_key(9), "date.month.september");
  assert_eq!(month_key(10), "date.month.october");
  assert_eq!(month_key(11), "date.month.november");
  assert_eq!(month_key(12), "date.month.december");
  assert_eq!(month_key(0), "date.month.january");
  assert_eq!(month_key(13), "date.month.january");
}
