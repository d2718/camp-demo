/*!
All the extra monstrosity demanded for report writing.

CREATE TABLE nmr (
    id      BIGINT,
    status  TEXT    /* one of { NULL, 'm', 'r' } */
);

CREATE TABLE facts (
    uname   TEXT REFERENCES students(uname),
    sem     TEXT,    /* one of { 'Fall', 'Spring', 'Summer' } */
    add     TEXT,
    sub     TEXT,
    mul     TEXT,
    div     TEXT
);

CREATE TABLE social (
    uname   TEXT REFERENCES students(uname),
    sem     TEXT,   /* one of { 'Fall', 'Spring', 'Summer' } */
    trait   TEXT,
    score   TEXT    /* 1- (worst) to 3+ (best) */
);

CREATE TABLE completion (
    uname   TEXT,
    sem     TEXT,   /* one of { 'Fall', 'Spring', 'Summer' } */
    courses TEXT
);
*/