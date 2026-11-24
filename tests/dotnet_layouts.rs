//! The console program of the Code tab for every line layout of its input and output
//! (`speci_local::ioplan`): the generated program reads the text the generator writes for the
//! test case, and prints what the test case expects. Needs the .NET SDK, see `common/mod.rs`.

use speci_local::csharp::{self, Inputs};
use speci_local::ioplan::{IoLayout, Level};
use speci_local::stuki::model::Program;

mod common;
use common::{run, serial};

/// A record per student; `db` counts the ones with the mark `m`.
const STUDENTS: &str = "Be:\nn∈N,\nm∈N,\nhallgatok∈Hallgato[1..n],\nHallgato=(nev:S x jegy:N)\nKi:\ndb∈N\nEf:\n-\nUf:\ndb=DARAB(i=1..n, hallgatok[i].jegy=m)\n";
const STUDENTS_TEST: &str = "n: 3\nm: 5\nhallgatok:\n  - nev: Anna\n    jegy: 5\n  - nev: Bence\n    jegy: 3\n  - nev: Csilla\n    jegy: 5\ndb: 2\n";

/// The students with the mark `m` are the output: records, and a count to share a line with.
const PICKED: &str = "Be:\nn∈N,\nm∈N,\nhallgatok∈Hallgato[1..n],\nHallgato=(nev:S x jegy:N)\nKi:\ndb∈N,\ny∈Hallgato[1..db]\nEf:\n-\nUf:\n(db,y)=KIVÁLOGAT(i=1..n, hallgatok[i].jegy=m, hallgatok[i])\n";
const PICKED_TEST: &str = "n: 3\nm: 5\nhallgatok:\n  - nev: Anna\n    jegy: 5\n  - nev: Bence\n    jegy: 3\n  - nev: Csilla\n    jegy: 5\ndb: 2\ny:\n  - nev: Anna\n    jegy: 5\n  - nev: Csilla\n    jegy: 5\n";

/// A table: `b` is its last element.
const TABLE: &str = "Be:\nn∈N,\nm∈N,\nt∈Z[1..n,1..m]\nKi:\nb∈Z\nEf:\n-\nUf:\nb=t[n,m]\n";
const TABLE_TEST: &str = "n: 2\nm: 3\nt: [[1, 5, 2], [7, 0, 3]]\nb: 3\n";

/// The numbers: `szamok` is read after `n`, `db` counts the odd ones; the picked ones are printed.
const NUMBERS: &str = "Be:\nn∈N,\nt∈Z[1..n]\nKi:\ndb∈N,\ny∈Z[1..db]\nEf:\n-\nUf:\n(db,y)=KIVÁLOGAT(i=1..n, t[i]>0, t[i])\n";
const NUMBERS_TEST: &str = "n: 5\nt: [3, -1, 0, 7, 2]\ndb: 3\ny: [3, 7, 2]\n";

/// One record (with a truth value in it), and a table of records.
const PERSON: &str = "Be:\nszemely∈(nev:S x kor:N x felnott:L)\nKi:\nok∈L\nEf:\n-\nUf:\nok=(szemely.kor>=18 és szemely.felnott)\n";
const PERSON_TEST: &str = "szemely: {nev: Anna, kor: 20, felnott: true}\nok: true\n";
const CELLS: &str =
    "Be:\nn∈N,\nm∈N,\nr∈Cella[1..n,1..m],\nCella=(a:N x b:N)\nKi:\ns∈N\nEf:\n-\nUf:\ns=r[n,m].b\n";
const CELLS_TEST: &str =
    "n: 2\nm: 2\nr: [[{a: 1, b: 2}, {a: 3, b: 4}], [{a: 5, b: 6}, {a: 7, b: 8}]]\ns: 8\n";

fn layout(items: &[(&str, Level, bool)]) -> IoLayout {
    let mut l = IoLayout::default();
    for (name, level, join) in items {
        l.set_level(name, *level);
        l.set_join(name, *join);
    }
    l
}

/// Builds the console program for `layout`, runs it on the generated input, and compares what it
/// prints with the expected output (written for the same layout).
fn round_trip(spec: &str, test: &str, what: &str, layout: &IoLayout) {
    let c = match csharp::convert_with(
        spec,
        "T",
        &Program::default(),
        test,
        Inputs::Console,
        layout,
    ) {
        Ok(c) => c,
        Err(csharp::ConvertError::Spec(e) | csharp::ConvertError::Test(e)) => {
            panic!("{what}: conversion failed: {e}")
        }
    };
    let parsed = speci_local::parse(spec).expect("the specification parses");
    let stdin = csharp::console_input_with(&parsed, test, layout).expect("input");
    let expected = csharp::console_output_with(&parsed, test, layout).expect("output");
    assert_eq!(c.stdin.as_deref(), Some(stdin.as_str()), "{what}");
    let f = run(&c.code, &stdin);
    assert!(
        csharp::same_output(&expected, &f.output),
        "{what}\n--- input\n{stdin}--- expected\n{expected}--- printed\n{}\n{}\n--- program\n{}",
        f.output,
        f.build,
        c.code
    );
}

#[test]
fn records_are_read_in_every_layout() {
    let Some(_guard) = serial() else { return };
    use Level::{Auto, Each, Rows, Single};
    for (what, l) in [
        ("default", IoLayout::default()),
        (
            "records in one line",
            layout(&[("hallgatok", Single, false)]),
        ),
        ("a field per line", layout(&[("hallgatok", Each, false)])),
        ("n m in one line", layout(&[("m", Auto, true)])),
        (
            "everything in one line",
            layout(&[("m", Auto, true), ("hallgatok", Single, true)]),
        ),
        (
            "a record per line, asked for",
            layout(&[("hallgatok", Rows, false)]),
        ),
    ] {
        round_trip(STUDENTS, STUDENTS_TEST, what, &l);
    }
}

#[test]
fn records_are_printed_in_every_layout() {
    let Some(_guard) = serial() else { return };
    use Level::{Auto, Each, Rows, Single};
    for (what, l) in [
        ("default", IoLayout::default()),
        ("a record per line", layout(&[("y", Rows, false)])),
        ("a field per line", layout(&[("y", Each, false)])),
        (
            "db and the records in one line",
            layout(&[("y", Single, true)]),
        ),
        (
            "input and output layouts together",
            layout(&[
                ("m", Auto, true),
                ("hallgatok", Each, false),
                ("y", Rows, false),
            ]),
        ),
    ] {
        round_trip(PICKED, PICKED_TEST, what, &l);
    }
}

#[test]
fn tables_are_read_in_every_layout() {
    let Some(_guard) = serial() else { return };
    use Level::{Auto, Each, Single};
    for (what, l) in [
        ("default", IoLayout::default()),
        ("table in one line", layout(&[("t", Single, false)])),
        ("a number per line", layout(&[("t", Each, false)])),
        ("n m in one line", layout(&[("m", Auto, true)])),
        (
            "everything in one line",
            layout(&[("m", Auto, true), ("t", Single, true)]),
        ),
    ] {
        round_trip(TABLE, TABLE_TEST, what, &l);
    }
}

#[test]
fn lists_are_read_and_printed_in_every_layout() {
    let Some(_guard) = serial() else { return };
    use Level::{Each, Single};
    for (what, l) in [
        ("default", IoLayout::default()),
        (
            "a number per line",
            layout(&[("t", Each, false), ("y", Each, false)]),
        ),
        ("n and the list in one line", layout(&[("t", Single, true)])),
        (
            "db and the list in one line",
            layout(&[("y", Single, true)]),
        ),
        ("both", layout(&[("t", Single, true), ("y", Single, true)])),
    ] {
        round_trip(NUMBERS, NUMBERS_TEST, what, &l);
    }
}

#[test]
fn a_single_record_is_read_in_every_layout() {
    let Some(_guard) = serial() else { return };
    for (what, l) in [
        ("default", IoLayout::default()),
        (
            "a field per line",
            layout(&[("szemely", Level::Each, false)]),
        ),
        ("one line", layout(&[("szemely", Level::Single, false)])),
    ] {
        round_trip(PERSON, PERSON_TEST, what, &l);
    }
}

#[test]
fn tables_of_records_are_read_in_every_layout() {
    let Some(_guard) = serial() else { return };
    use Level::{Auto, Each, Rows, Single};
    for (what, l) in [
        ("default: a row per line", IoLayout::default()),
        ("a row per line, asked for", layout(&[("r", Rows, false)])),
        ("one line", layout(&[("r", Single, false)])),
        ("a field per line", layout(&[("r", Each, false)])),
        (
            "n m and the table in one line",
            layout(&[("m", Auto, true), ("r", Single, true)]),
        ),
    ] {
        round_trip(CELLS, CELLS_TEST, what, &l);
    }
}

/// The test values mode (no console) handles records too.
#[test]
fn records_with_the_test_values_written_into_the_program() {
    let Some(_guard) = serial() else { return };
    for (spec, test, printed) in [
        (PERSON, PERSON_TEST, "ok = True"),
        (CELLS, CELLS_TEST, "s = 8"),
        (STUDENTS, STUDENTS_TEST, "db = 2"),
        (PICKED, PICKED_TEST, "y = Anna 5 Csilla 5"),
    ] {
        let c = csharp::convert(spec, "T", &Program::default(), test, Inputs::Test)
            .unwrap_or_else(|_| panic!("conversion failed: {spec}"));
        let f = run(&c.code, "");
        assert!(
            f.output.contains(printed),
            "{printed}\n{}\n{}\n{}",
            f.output,
            f.build,
            c.code
        );
    }
}
