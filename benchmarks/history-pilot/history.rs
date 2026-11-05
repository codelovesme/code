// Same ASCII payloads, retained histories and full-content checks as Code/C.
#[cfg(cow)]
use std::rc::Rc;
const PAYLOAD: usize = __PAYLOAD__;
const SECTIONS: usize = __SECTIONS__;
const DOCUMENT: bool = __DOCUMENT__;
const VERSIONS: usize = 64;
#[derive(Clone)]
struct Section {
    enabled: bool,
    #[cfg(cow)]
    text: Rc<String>,
    #[cfg(not(cow))]
    text: String,
}
#[cfg(cow)]
type State = Rc<Vec<Rc<Section>>>;
#[cfg(not(cow))]
type State = Vec<Section>;
fn main() {
    let original = "a".repeat(PAYLOAD);
    #[cfg(cow)]
    let payload = Rc::new(original);
    #[cfg(not(cow))]
    let payload = original;
    #[cfg(cow)]
    let mut current: State = Rc::new(
        (0..SECTIONS)
            .map(|_| {
                Rc::new(Section {
                    enabled: false,
                    text: payload.clone(),
                })
            })
            .collect(),
    );
    #[cfg(not(cow))]
    let mut current: State = (0..SECTIONS)
        .map(|_| Section {
            enabled: false,
            text: payload.clone(),
        })
        .collect();
    let mut snapshots = Vec::with_capacity(VERSIONS + 1);
    for v in 0..VERSIONS {
        snapshots.push(current.clone());
        #[cfg(cow)]
        let item = Rc::make_mut(&mut Rc::make_mut(&mut current)[if DOCUMENT { 0 } else { v }]);
        #[cfg(not(cow))]
        let item = &mut current[if DOCUMENT { 0 } else { v }];
        if DOCUMENT {
            #[cfg(cow)]
            let text = Rc::make_mut(&mut item.text);
            #[cfg(not(cow))]
            let text = &mut item.text;
            text.replace_range(v..v + 1, "b");
        } else {
            item.enabled = true;
        }
    }
    snapshots.push(current);
    let mut expected = payload.as_str().to_owned();
    for (v, saved) in snapshots.iter().enumerate() {
        assert_eq!(saved.len(), SECTIONS);
        for (i, item) in saved.iter().enumerate() {
            assert_eq!(item.enabled, !DOCUMENT && i < v);
            #[cfg(cow)]
            let equal =
                (!DOCUMENT && Rc::ptr_eq(&item.text, &payload)) || item.text.as_str() == expected;
            #[cfg(not(cow))]
            let equal = item.text == expected;
            assert!(equal);
        }
        if DOCUMENT && v < VERSIONS {
            expected.replace_range(v..v + 1, "b");
        }
    }
    assert_eq!(snapshots.len(), VERSIONS + 1);
}
