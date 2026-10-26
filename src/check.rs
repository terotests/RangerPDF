//! A small test runner every target can run: each test names the parity
//! feature it covers, and prints `PASS <feature> <test>` or
//! `FAIL <feature> <test>: <what>`.

pub struct Check {
    pub passed: i64,
    pub failed: i64,
    feature: String,
    name: String,
    problems: Vec<String>,
    open: bool,
}

impl Check {
    pub fn new() -> Check {
        Check { passed: 0, failed: 0, feature: String::new(), name: String::new(), problems: vec![], open: false }
    }

    fn close(&mut self) {
        if !self.open {
            return;
        }
        // `feature` may name several ids, separated by spaces
        let ids: Vec<&str> = self.feature.split(' ').collect();
        if self.problems.is_empty() {
            self.passed += 1;
            for id in ids.iter() {
                if !id.is_empty() {
                    println!("PASS {} {}", id, self.name);
                }
            }
        } else {
            self.failed += 1;
            for id in ids.iter() {
                if id.is_empty() {
                    continue;
                }
                for p in self.problems.iter() {
                    println!("FAIL {} {}: {}", id, self.name, p);
                }
            }
        }
        self.problems = vec![];
        self.open = false;
    }

    /// Starts a test of `feature`: one or more ids separated by spaces,
    /// gql:<spec item> or mf:<feature>.
    pub fn test(&mut self, feature: &str, name: &str) {
        self.close();
        self.feature = feature.to_string();
        self.name = name.to_string();
        self.open = true;
    }

    pub fn eq(&mut self, what: &str, got: &str, want: &str) {
        if got != want {
            self.problems.push(format!("{}: got [{}] want [{}]", what, got, want));
        }
    }

    pub fn eqi(&mut self, what: &str, got: i64, want: i64) {
        if got != want {
            self.problems.push(format!("{}: got {} want {}", what, got, want));
        }
    }

    pub fn ok(&mut self, what: &str, cond: bool) {
        if !cond {
            self.problems.push(format!("{}: false", what));
        }
    }

    /// Ends the run; true when every test passed.
    pub fn done(&mut self) -> bool {
        self.close();
        if self.failed == 0 {
            println!("ALL PASS {}", self.passed);
            return true;
        }
        println!("{} FAILED, {} passed", self.failed, self.passed);
        false
    }
}
