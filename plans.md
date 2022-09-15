
# Database Design

## Auth

Separate database from the rest of the sections.

```sql

CREATE TABLE users (
    uname TEXT PRIMARY KEY,
    hash  TEXT
);

CREATE TABLE keys (
    key       TEXT,
    uname     TEXT REFERENCES users,
    last_used TIMESTAMP
);
```

## Users

```sql

CREATE TABLE users (
    uname TEXT PRIMARY KEY,
    role  TEXT,      /* one of { 'admin', 'boss', 'teacher', 'student' } */
    salt  TEXT,
    email TEXT
);

/* users.role should properly be an ENUM, but that type doesn't seem to
 * be well-supported by the tokio-postgres crate.
 */
 
CREATE TABLE teachers (
    uname TEXT REFERENCES users(uname),
    name  TEXT
);

CREATE TABLE students (
    uname   TEXT REFERENCES users(uname),
    last    TEXT,
    rest    TEXT,
    teacher TEXT REFERENCES teachers(uname),
    parent  TEXT,    /* parent email address */
    fall_exam            TEXT,
    spring_exam          TEXT,
    fall_exam_fraction   REAL,
    spring_exam_fraction REAL,
    fall_notices         SMALLINT,
    spring_notices       SMALLINT
);

```

## Courses

```sql

CREATE TABLE courses (
    id    SERIAL PRIMARY KEY,
    sym   TEXT UNIQUE NOT NULL,
    book  TEXT,
    title TEXT NOT NULL,
    level REAL
);

CREATE TABLE chapters (
    id       BIGSERIAL PRIMARY KEY,
    course   INTEGER REFERENCES courses(id),
    sequence SMALLINT,
    title    TEXT,     /* null should give default-generated title */
    weight   REAL     /* null should give default value of 1.0 */
);

CREATE TABLE custom_chapters (
    id    BIGSERIAL PRIMARY KEY,
    uname REFERENCES user(uname),   /* username of creator */
    title TEXT NOT NULL,
    weight REAL     /* null should give default value of 1.0 */
);

```

## Calendar

```sql

CREATE TABLE calendar (
    day DATE
);
```

## Goals

```sql

CREATE TABLE goals (
    id BIGSERIAL PRIMARY KEY,
    uname   	TEXT REFERENCES students(uname),
    sym     	TEXT REFERENCES courses(sym),
	seq			SMALLINT REFERENCES chapters(sequence),
    custom  	BIGINT REFERENCES custom_chapters(id),
    review      BOOL,
    incomplete  BOOL,
    due		    DATE,
    done	    DATE,
    tries 		SMALLINT,          /* null means 1 if complete */
    score TEXT
);

```

## Reporting Extras

This information might even live in a separate database?

```sql

CREATE TABLE student_misc (
    uname TEXT PRIMARY KEY,
    /* each fall social-emotional score */
    /* each spring social-emotional score */
    fall_complete
    spring_complete
    fall_courses_complete
    spring_courses_complete
    fall_goals_remaining
    spring_goals_remaining
);
```

# API

### `/`
A `GET` presents a page with the login form. The form has

  * `<input type="text" name="uname">`
  * `<input type="password" name="password">`
The form action `POST`s the login info to `/login`.

### `/login`
Accepts the login `POST` from the `<form>` on `/`; attempts to log the user in.

Will serve a login error or the appropriate page for that `User` type.

### `/static`
Maps to the directory of static assets that should just be served as-is.

### `/admin`
The `Admin` API endpoint.

`POST`s to this resource should specify authentication headers

  * `x-camp-uname` the uname of the Admin making the request
  * `x-camp-key` the auth key issued to the Admin upon login
as well as at least

  * `x-camp-action` which should speciy which action the Admin is attempting
    to take with this request.
  * `x-camp-request-id`, which should be returned with all responses
    so that the frontend can track which requests have been completed.

Other information will be `x-camp-action` specific, and will mostly consist
of `content-type: application/json` bodies.

```text
x-camp-action: populate-admins
x-camp-action: populate-teachers
x-camp-action: populate-students
x-camp-action: populate-courses
```
Return data about the specified objects in JSON format, with the same
`x-camp-action` response header, and `content-type: application/json`.

```text
x-camp-action: add-admin
x-camp-action: add-teacher
x-camp-action: add-student
```
Add the appropriate type of `User` from data specified in the JSON body.
Response should be identical to an `x-camp-action: populate-xxx` request.

`x-camp-action: delete-user`

Accompanied by the `x-camp-delete-uname` header, this should delete the
given `User`, and return the appropriate `x-camp-action: populate-xxx`
response based on `User` type.

`x-camp-action: upload-students`

Insert multiple students based on the contents of the
`content-type: text/csv` body. Should return a
`x-camp-action: populate-students` response.

`x-camp-action: insert-course`

Insert a new, chapterless `Course` based on the
`content-type: application/json` body. Response should be the same
as `x-camp-action: populate-courses`.

`x-camp-action: delete-course`

Along with the `x-camp-delete-id` header, remove the specified `Course` and
all of its chapters from the database, and return the
`x-camp-action: populate-courses` response.

This should fail if there are any students who currently have goals
from the given course.

`x-camp-action: insert-chapter`

Insert a new `Chapter` to an extant `Course` based on the JSON body.
Response should be `x-camp-action: single-course` and contain a JSON
body of information just to replace that one course.

`x-camp-action: delete-chapter`

Along with the `x-camp-delete-id` header, remove the specified `Chapter`
from the database, returning the appropriate `x-camp-action: single-course`
response.

This should fail if there are any students who currently have this chapter
as a goal.

`x-camp-action: upload-course`

Insert a new course, and all of its chapters, based on the contents of
the hybrid TOML/CSV body (should be `content-type: text/plain`).
Respose should be the same as `x-camp-action: populate-courses`.