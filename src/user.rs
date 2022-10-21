/*!
Internal representations of the four types of users of this system:
  * [`Admin`](User::Admin): responsible for adding users and courses and updating
    the calendar
  * [`Boss`](User::Boss): can see all students' progress, and autogenerate (and send)
    emails to their parents about their status.
  * [`Teacher`]: can see a subset of students' progress (theirs), and
    add and update their goal status
  * [`Student`]: can see their own progress

Most of the information contained herein is just directly wrapped data from
the underlying Postgres store, collected and cross-referenced.
*/
use std::cmp::Ordering;
use std::io::Read;

use serde::{Deserialize, Serialize};

/// Marks the role of the [`User`].
///
/// The `User` is a sum type, but this distinction is useful elsewhere.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Role {
    Admin,
    Boss,
    Teacher,
    Student,
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let token = match self {
            Role::Admin => "Admin",
            Role::Boss => "Boss",
            Role::Teacher => "Teacher",
            Role::Student => "Student",
        };

        write!(f, "{}", token)
    }
}

impl std::str::FromStr for Role {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Admin" => Ok(Role::Admin),
            "Boss" => Ok(Role::Boss),
            "Teacher" => Ok(Role::Teacher),
            "Student" => Ok(Role::Student),
            _ => Err(format!("{:?} is not a valid Role.", s)),
        }
    }
}

/// Information common to all users.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct BaseUser {
    /// Uniquely identifies each user.
    pub uname: String,
    /// Helpful here because the `BaseUser` is information common to all
    /// four types of users.
    pub role: Role,
    /// Salt string for comparing a supplied password with the hash on
    /// record in the [auth database](crate::auth::Db).
    pub salt: String,
    /// Used largely to verify identity when resetting a password.
    pub email: String,
}

impl BaseUser {
    /**
    Assign the `BaseUser` the given `Role`.

    This can be necessary when deserializing or wrapping data from a source
    that doesn't explicitly have this information, but the `Role` is known
    in context (such as the various `BaseUser::into_xxx()` methods).
    */
    fn rerole(self, role: Role) -> BaseUser {
        BaseUser {
            uname: self.uname,
            role,
            salt: self.salt,
            email: self.email,
        }
    }
    pub fn into_admin(self) -> User {
        User::Admin(self.rerole(Role::Admin))
    }
    pub fn into_boss(self) -> User {
        User::Boss(self.rerole(Role::Boss))
    }
    pub fn into_teacher(self, name: String) -> User {
        User::Teacher(Teacher {
            base: self.rerole(Role::Teacher),
            name,
        })
    }
    #[allow(clippy::too_many_arguments)]
    pub fn into_student(
        self,
        last: String,
        rest: String,
        teacher: String,
        parent: String,
        fall_exam: Option<String>,
        spring_exam: Option<String>,
        fall_exam_fraction: f32,
        spring_exam_fraction: f32,
        fall_notices: i16,
        spring_notices: i16,
    ) -> User {
        let s = Student {
            base: self.rerole(Role::Student),
            last,
            rest,
            teacher,
            parent,
            fall_exam,
            spring_exam,
            fall_exam_fraction,
            spring_exam_fraction,
            fall_notices,
            spring_notices,
        };
        User::Student(s)
    }
}

/// Wraps Teacher info.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Teacher {
    /// uname, salt, email
    pub base: BaseUser,
    /// Display name.
    pub name: String,
}

/**
Wraps all information about a student except for pace goals.
*/
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Student {
    pub base: BaseUser,
    /// Last name of the student.
    pub last: String,
    /// The rest of the student's name (first, middle initial, etc.).
    pub rest: String,
    /// `uname` of the student's teacher.
    pub teacher: String,
    /// Parent email address(es? if possible?).
    pub parent: String,
    /// Mark of Fall Semester Exam (if complete).
    pub fall_exam: Option<String>,
    /// Mark of Spring Semester Exam (if complete).
    pub spring_exam: Option<String>,
    /// Fall Exam counts for this portion of the Fall Semester grade.
    pub fall_exam_fraction: f32,
    /// Spring Exam counts for this portion of the Spring Semester grade.
    pub spring_exam_fraction: f32,
    /// Number of homework notices that "count" for the Fall Semester.
    pub fall_notices: i16,
    /// Number of homework notices that "count" for the Spring Semester.
    pub spring_notices: i16,
}

impl Student {
    /**
    Student .csv rows should look like this

    ```csv
    #uname, last,   rest, email,                    parent,                 teacher
    jsmith, Smith,  John, lil.j.smithy@gmail.com,   js.senior@gmail.com,    jenny
    ```
    */
    pub fn from_csv_line(row: &csv::StringRecord) -> Result<Student, &'static str> {
        log::trace!("Student::from_csv_line( {:?} ) called.", row);

        let uname = match row.get(0) {
            Some(s) => s.to_owned(),
            None => {
                return Err("no uname");
            }
        };
        let email = match row.get(3) {
            Some(s) => s.to_owned(),
            None => {
                return Err("no email address");
            }
        };

        let base = BaseUser {
            uname,
            role: Role::Student,
            salt: String::new(),
            email,
        };

        let last = match row.get(1) {
            Some(s) => s.to_owned(),
            None => {
                return Err("no last name");
            }
        };
        let rest = match row.get(2) {
            Some(s) => s.to_owned(),
            None => {
                return Err("no rest of name");
            }
        };
        let teacher = match row.get(5) {
            Some(s) => s.to_owned(),
            None => {
                return Err("no teacher uname");
            }
        };
        let parent = match row.get(4) {
            Some(s) => s.to_owned(),
            None => {
                return Err("no parent email");
            }
        };

        let stud = Student {
            base,
            last,
            rest,
            teacher,
            parent,
            fall_exam: None,
            spring_exam: None,
            fall_exam_fraction: 0.2_f32,
            spring_exam_fraction: 0.2_f32,
            fall_notices: 0,
            spring_notices: 0,
        };
        Ok(stud)
    }

    /**
    Create a `Vec` of `Student`s from CSV formatted information.

    This is meant for adding multiple new students to the database at once;
    the CSV format lacks a lot of fields, which will automatically be set
    to default "starting the year" values upon insertion.

    Example CSV format is
      1. `uname` (`Student.base.uname`)
      2. last name (`Student.last` field)
      3. rest of name (`Student.rest` field)
      4. student email address (`Student.base.email` field)
      5. parent email address (`Student.parent` field)
      6. student's teacher's uname (`Student.teacher` field)

    Blank lines and lines beginning with `#` are ignored.

    An example row:

    ```csv
    #uname, last,   rest, email,                    parent,                 teacher
    jsmith, Smith,  John, lil.j.smithy@gmail.com,   js.senior@gmail.com,    jenny
    ```
    */
    pub fn vec_from_csv_reader<R: Read>(r: R) -> Result<Vec<Student>, String> {
        log::trace!("Student::vec_from_csv_reader(...) called.");

        let mut csv_reader = csv::ReaderBuilder::new()
            .comment(Some(b'#'))
            .trim(csv::Trim::All)
            .flexible(false)
            .has_headers(false)
            .from_reader(r);

        // We overestimate the amount of `Student`s required and then
        // shrink it later.
        let mut students: Vec<Student> = Vec::with_capacity(256);

        for (n, res) in csv_reader.records().enumerate() {
            match res {
                Ok(record) => match Student::from_csv_line(&record) {
                    Ok(stud) => {
                        students.push(stud);
                    }
                    Err(e) => {
                        let estr = match record.position() {
                            Some(p) => format!("Error on line {}: {}", p.line(), &e),
                            None => format!("Error in CSV record {}: {}", &n, &e),
                        };
                        return Err(estr);
                    }
                },
                Err(e) => {
                    let estr = match e.position() {
                        Some(p) => format!("Error on line {}: {}", p.line(), &e),
                        None => format!("Error in CSV record {}: {}", &n, &e),
                    };
                    return Err(estr);
                }
            }
        }

        students.shrink_to_fit();
        log::trace!(
            "Students::vec_from_csv_reader() returns {} Students.",
            students.len()
        );
        Ok(students)
    }
}

/// Sum type unifying all four types of users.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub enum User {
    Admin(BaseUser),
    Boss(BaseUser),
    Teacher(Teacher),
    Student(Student),
}

impl User {
    pub fn uname(&self) -> &str {
        match self {
            User::Admin(base) => &base.uname,
            User::Boss(base) => &base.uname,
            User::Teacher(t) => &t.base.uname,
            User::Student(s) => &s.base.uname,
        }
    }

    pub fn salt(&self) -> &str {
        match self {
            User::Admin(base) => &base.salt,
            User::Boss(base) => &base.salt,
            User::Teacher(t) => &t.base.salt,
            User::Student(s) => &s.base.salt,
        }
    }

    pub fn email(&self) -> &str {
        match self {
            User::Admin(base) => &base.email,
            User::Boss(base) => &base.email,
            User::Teacher(t) => &t.base.email,
            User::Student(s) => &s.base.email,
        }
    }

    pub fn role(&self) -> Role {
        match self {
            User::Admin(_) => Role::Admin,
            User::Boss(_) => Role::Boss,
            User::Teacher(_) => Role::Teacher,
            User::Student(_) => Role::Student,
        }
    }
}

impl PartialOrd for User {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let x = match self {
            User::Admin(ref a) => match other {
                User::Admin(ref oa) => a.uname.cmp(&oa.uname),
                _ => Ordering::Less,
            },
            User::Boss(ref b) => match other {
                User::Admin(_) => Ordering::Greater,
                User::Boss(ref ob) => b.uname.cmp(&ob.uname),
                _ => Ordering::Less,
            },
            User::Teacher(ref t) => match other {
                User::Teacher(ref ot) => t.base.uname.cmp(&ot.base.uname),
                User::Student(_) => Ordering::Less,
                _ => Ordering::Greater,
            },
            User::Student(ref s) => match other {
                User::Student(ref os) => match s.last.cmp(&os.last) {
                    Ordering::Equal => s.rest.cmp(&os.rest),
                    x => x,
                },
                _ => Ordering::Greater,
            },
        };
        Some(x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::ensure_logging;

    #[test]
    fn students_from_csv() {
        ensure_logging();
        let f = std::fs::File::open("test/good_students_0.csv").unwrap();
        let studs = Student::vec_from_csv_reader(f).unwrap();
        log::trace!("Students:\n{:#?}", &studs);
    }

    #[test]
    fn make_users_serialized() {
        use serde_json::to_writer_pretty;

        let base = BaseUser {
            uname: "aguy".to_owned(),
            role: Role::Admin,
            salt: "asdf".to_owned(),
            email: "guy@dude.net".to_owned(),
        };

        let a = base.clone().into_admin();
        let b = base.clone().into_boss();
        let t = base.clone().into_teacher("Alfred Guy".to_owned());
        let s = base.clone().into_student(
            "Guy".to_owned(),
            "Alfred C.".to_owned(),
            "mrt".to_owned(),
            "old.guy@gmail.com".to_owned(),
            None,
            None,
            0.2,
            0.2,
            0,
            0,
        );

        println!("Debug:\n{:#?}\n{:#?}\n{:#?}\n{:#?}\n\n", &a, &b, &t, &s);

        let mut buff: Vec<u8> = Vec::new();
        buff.extend_from_slice(b"serde_json:\n");
        to_writer_pretty(&mut buff, &a).unwrap();
        buff.push(b'\n');
        to_writer_pretty(&mut buff, &b).unwrap();
        buff.push(b'\n');
        to_writer_pretty(&mut buff, &t).unwrap();
        buff.push(b'\n');
        to_writer_pretty(&mut buff, &s).unwrap();
        buff.push(b'\n');
        let buff = String::from_utf8(buff).unwrap();

        println!("{}", &buff);
    }
}
