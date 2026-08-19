//! `compute.series.drivers` (§4): (subject, metric) series over as-of time
//! with normalized stamps; **moved-only** diffs — "a row inserted is not a
//! value changed" (the monitor task's distinctive rule).

use rein_core::time::Timestamp;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DriverPoint {
    pub as_of: Timestamp,
    pub value: f64,
    pub unit: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DriverSeries {
    pub subject: String,
    pub metric: String,
    pub points: Vec<DriverPoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MovedValue {
    pub as_of: Timestamp,
    pub prior: f64,
    pub new: f64,
    pub unit: String,
}

/// A moved-only diff between two vintages of the same series.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeriesDiff {
    pub subject: String,
    pub metric: String,
    /// Values that *changed* at an as-of both vintages carry.
    pub moved: Vec<MovedValue>,
    /// New as-ofs — reported separately, never counted as movement.
    pub inserted: Vec<DriverPoint>,
    /// As-ofs present before, absent now — a disappearance is surfaced too.
    pub removed: Vec<DriverPoint>,
}

pub fn diff(prior: &DriverSeries, new: &DriverSeries) -> SeriesDiff {
    let mut moved = Vec::new();
    let mut inserted = Vec::new();
    let mut removed = Vec::new();

    for np in &new.points {
        match prior.points.iter().find(|pp| pp.as_of == np.as_of) {
            Some(pp) => {
                if (pp.value - np.value).abs() > f64::EPSILON.max(pp.value.abs() * 1e-12) {
                    moved.push(MovedValue {
                        as_of: np.as_of,
                        prior: pp.value,
                        new: np.value,
                        unit: np.unit.clone(),
                    });
                }
            }
            None => inserted.push(np.clone()),
        }
    }
    for pp in &prior.points {
        if !new.points.iter().any(|np| np.as_of == pp.as_of) {
            removed.push(pp.clone());
        }
    }

    SeriesDiff {
        subject: new.subject.clone(),
        metric: new.metric.clone(),
        moved,
        inserted,
        removed,
    }
}
