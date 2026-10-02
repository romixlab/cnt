//! Reading and clearing `Counters` instances, on a host.
#![cfg(not(feature = "disabled"))]

#[derive(cnt::Count)]
enum Event {
    A,
    #[count(u64)]
    B,
}

static RAM: cnt::Counters<Event> = cnt::counters!(Event, ram);
static BKP: cnt::Counters<Event> = cnt::bkp_counters!(Event, bkp);
static UNUSED: cnt::Counters<Event> = cnt::counters!(Event, unused);

#[test]
fn get_and_clear() {
    for cnt in [&RAM, &BKP] {
        cnt.count(Event::A);
        cnt.count(Event::A);
        cnt.add(Event::B, (1 << 32) + 3);
        assert_eq!(cnt.get(Event::A), 2);
        assert_eq!(cnt.get(Event::B), (1 << 32) + 3);

        cnt.clear();
        assert_eq!(cnt.get(Event::A), 0);
        assert_eq!(cnt.get(Event::B), 0);
        cnt.count(Event::B);
        assert_eq!(cnt.get(Event::B), 1);
    }
}

#[test]
fn never_counted_is_zero() {
    assert_eq!(UNUSED.get(Event::A), 0);
    assert_eq!(UNUSED.get(Event::B), 0);
}
