//! Tests running in parallel threads, each with its own `Counters` static, do not see each other's counts.
#![cfg(not(feature = "disabled"))]

#[derive(cnt::Count)]
enum Event {
    A,
    #[count(u64)]
    B,
}

fn count_many(cnt: &'static cnt::Counters<Event>) {
    for _ in 0..10_000 {
        cnt.count(Event::A);
        cnt.add(Event::B, 2);
    }
}

macro_rules! isolated_test {
    ($($name:ident),*) => {$(
        #[test]
        fn $name() {
            // Same instance name in every test, as a copied test would have
            static CNT: cnt::Counters<Event> = cnt::counters!(Event, test);
            count_many(&CNT);
            assert_eq!(CNT.get(Event::A), 10_000);
            assert_eq!(CNT.get(Event::B), 20_000);
        }
    )*};
}

isolated_test!(t0, t1, t2, t3, t4, t5, t6, t7);
