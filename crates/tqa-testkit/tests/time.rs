//! Deterministic clocks wake all due sleepers and retain requested durations.

use std::{error::Error, time::Duration};
use tqa_llm::Jitter;
use tqa_testkit::{FakeClock, FakeSleeper, FixedJitter};
use tqa_types::{Clock, Sleeper};

#[test]
fn fake_clock_wakes_sleepers_only_at_their_deadlines() -> Result<(), Box<dyn Error>> {
    let clock = FakeClock::new();
    let before = clock.now();
    let sleeper = FakeSleeper::new(clock.clone());
    let mut first = tokio_test::task::spawn(sleeper.sleep(Duration::from_secs(1)));
    let mut second = tokio_test::task::spawn(sleeper.sleep(Duration::from_secs(2)));
    assert!(first.poll().is_pending());
    assert!(second.poll().is_pending());
    clock.advance(Duration::from_secs(1))?;
    assert!(first.poll().is_ready());
    assert!(second.poll().is_pending());
    clock.advance(Duration::from_secs(1))?;
    assert!(second.poll().is_ready());
    assert_eq!(clock.now().duration_since(before), Duration::from_secs(2));
    assert_eq!(
        sleeper.durations(),
        [Duration::from_secs(1), Duration::from_secs(2)]
    );
    Ok(())
}

#[test]
fn fixed_jitter_validates_and_repeats_the_sequence() -> Result<(), Box<dyn Error>> {
    let mut jitter = FixedJitter::sequence(vec![0.0, 0.5])?;
    assert_eq!(
        [
            jitter.next_fraction(),
            jitter.next_fraction(),
            jitter.next_fraction()
        ]
        .map(f64::to_bits),
        [0.0_f64, 0.5, 0.0].map(f64::to_bits)
    );
    for value in [-0.1, 1.0, f64::NAN, f64::INFINITY] {
        assert!(FixedJitter::new(value).is_err());
    }
    assert!(FixedJitter::sequence(vec![]).is_err());
    Ok(())
}
