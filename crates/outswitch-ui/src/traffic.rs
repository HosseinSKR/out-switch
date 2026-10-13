#[derive(Clone,Copy,Debug,Default)]
pub struct Sample {pub time:u64,pub down:f64,pub up:f64,pub received:u64,pub sent:u64}
pub fn combined(history:&std::collections::BTreeMap<String,std::collections::VecDeque<Sample>>)->Vec<Sample>{
    let mut points=std::collections::BTreeMap::<u64,Sample>::new();
    for sample in history.values().flatten(){let p=points.entry(sample.time).or_insert(Sample{time:sample.time,..Default::default()});p.down+=sample.down;p.up+=sample.up;p.received=p.received.saturating_add(sample.received);p.sent=p.sent.saturating_add(sample.sent);}
    let mut points:Vec<_>=points.into_values().collect();if points.len()>120{points.drain(..points.len()-120);}points
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn adapters_are_combined_by_timestamp_not_array_position(){
        let history=std::collections::BTreeMap::from([("old".into(),[Sample{time:10,down:4.,..Default::default()},Sample{time:12,down:6.,..Default::default()}].into()),("new".into(),[Sample{time:12,down:2.,..Default::default()},Sample{time:14,down:3.,..Default::default()}].into())]);
        let points=combined(&history);assert_eq!(points.iter().map(|p|(p.time,p.down)).collect::<Vec<_>>(),vec![(10,4.),(12,8.),(14,3.)]);
    }
}
