use sim_widget_rs::model::{DecodeLimits, ValueId};
use sim_widget_rs::{Document, OperatorRegistry, Session, SessionConfig, TimeTicks};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let document = Document::decode(
        include_bytes!("../tests/fixtures/cosines.json"),
        DecodeLimits::default(),
    )?;
    let mut session = Session::new(
        document,
        OperatorRegistry::standard()?,
        SessionConfig::default(),
    )?;
    let id = ValueId::new("c2")?;
    for (ticks, degrees) in [(0, 60), (127_008_000_000, 90), (254_016_000_000, 120)] {
        session.seek(session.revision(), TimeTicks::new(ticks))?;
        let frame = session.snapshot();
        println!(
            "theta={degrees} deg; c²={:.12}; ticks={}; actual operations={}",
            frame.truth()[&id].as_scalar()?,
            frame.time(),
            frame
                .provenance()
                .values()
                .filter(|p| p.operation.is_some())
                .count()
        );
    }
    Ok(())
}
