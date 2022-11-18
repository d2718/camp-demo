/*
admin.js

Frontend JS BS to make the admin's page work.

The util.js script must load before this one. It should be loaded
synchronously at the bottom of the <BODY>, and this should be
DEFERred.
*/
"use strict";

const API_ENDPOINT = "/admin";
const STATE = {
    error_count: 0
};
STATE.next_error = function() {
    const err = STATE.error_count;
    STATE.error_count += 1;
    return err;
}
const DATA = {
    users: new Map(),
    courses: new Map(),
    completion: new Map(),
};

const DISPLAY = {
    confirm: document.getElementById("are-you-sure"),
    confirm_message: document.querySelector("dialog#are-you-sure > p"),
    admin_tbody:   document.querySelector("table#admin-table > tbody"),
    admin_edit:    document.getElementById("alter-admin"),
    boss_tbody:    document.querySelector("table#boss-table > tbody"),
    boss_edit:     document.getElementById("alter-boss"),
    teacher_tbody: document.querySelector("table#teacher-table > tbody"),
    teacher_edit:  document.getElementById("alter-teacher"),
    student_tbody: document.querySelector("table#student-table > tbody"),
    student_edit:  document.getElementById("alter-student"),
    student_upload: document.getElementById("upload-students-dialog"),
    student_paste: document.getElementById("paste-students-dialog"),
    course_tbody:  document.querySelector("table#course-table > tbody"),
    course_edit:   document.getElementById("alter-course"),
    course_upload: document.getElementById("upload-course-dialog"),
    chapter_edit:  document.getElementById("alter-chapter"),
    student_reset: document.getElementById("reset-students"),
    history_year: document.querySelector("tbody#add-completion-history input[name='year']"),
};

function populate_users(r) {
    r.json()
    .then(j => {
        console.log("populate-users response:");
        console.log(j);

        DATA.users = new Map();
        UTIL.clear(DISPLAY.admin_tbody);
        UTIL.clear(DISPLAY.boss_tbody);
        UTIL.clear(DISPLAY.teacher_tbody);
        UTIL.clear(DISPLAY.student_tbody);
        for(const u of j) {
            add_user_to_display(u);
        }
    }).catch(RQ.add_err);
}

function populate_completion(r) {
    r.json()
    .then(j => {
        console.log("populate-completion response:");
        console.log(j);

        for(const [name, vec] of Object.entries(j)) {
            DATA.completion.set(name, vec);
        }
    }).catch(RQ.add_err);
}

function update_completion(r) {
    r.json()
    .then(j => {
        console.log("update-completion response:");
        console.log(j);

        const uname = r.headers.get("x-camp-student");
        DATA.completion.set(uname, j);
        display_completion_history(uname);
    }).catch(RQ.add_err);
}

function field_response(r) {
    if(!r.ok) {
        r.text()
        .then(t => {
            const err_txt = `${t}\n(${r.status}: ${r.statusText})`;
            RQ.add_err(err_txt);
        }
        ).catch(e => {
            const e_n = STATE.next_error();
            const err_txt = `Error #${e_n} (see console)`;
            console.log(e_n, e, r);
            RQ.add_err(err_txt);
        })

        return;
    }

    let action = r.headers.get("x-camp-action");

    if (!action) {
        const e_n = STATE.next_error();
        const err_txt = `Response lacked x-camp-action header. (See console error #${e_n}.)`;
        console.log(e_n, r);
        RQ.add_err(err_txt);
        return;
    }
    switch(action) {
        case "populate-users":
            populate_users(r); break;
        case "populate-courses":
            populate_courses(r); break;
        case "populate-completion":
            populate_completion(r); break;
        case "update-completion":
            update_completion(r); break;
        default:
            const e_n = STATE.next_error();
            const err_txt = `Unrecognized x-camp-action header: ${action}. (See console error #${e_n})`;
            console.log(e_n, r);
            RQ.add_err(err_txt);
            break;
    }
}

function request_action(action, body, description, extra_headers) {
    const headers = { "x-camp-action": action };
    if(extra_headers) {
        for(const [name, value] of Object.entries(extra_headers)) {
            headers[name] = value;
        }
    }
    const options = {
        method: "POST",
        headers: headers,
    };
    if(body) {
        const bt = typeof(body);
        if(bt == "string") {
            options.headers["content-type"] = "text/plain";
            options.body = body;
        } else if(bt == "object") {
            options.headers["content-type"] = "application/json"
            options.body = JSON.stringify(body);
        }
    }

    const r = new Request(
        API_ENDPOINT,
        options
    );

    const desc = (description || action);

    api_request(r, desc, field_response);
}

/*

USERS section

The functions and objects in this section are for dealing with Users,
that is, the stuff on the "Staff" and "Students" tabs.

*/

function make_user_edit_button_td(uname, edit_func) {
    const butt = document.createElement("button");
    butt.setAttribute("data-uname", uname);
    UTIL.label("edit", butt);
    butt.addEventListener("click", edit_func);
    const td = document.createElement("td");
    td.appendChild(butt);
    return td;
}

/*
Add user object to appropriate table. Also insert into the
DATA.users Map.
*/
function add_user_to_display(u) {
    console.log("adding user to display:", u);

    if (u.Admin) {
        const v = u.Admin;
        DATA.users.set(v.uname, u);

        const tr = document.createElement("tr");
        tr.setAttribute("data-uname", v.uname);
        tr.appendChild(UTIL.text_td(v.uname));
        tr.appendChild(UTIL.text_td(v.email));
        tr.appendChild(make_user_edit_button_td(v.uname, edit_admin));

        DISPLAY.admin_tbody.appendChild(tr);

    } else if(u.Boss) {
        const v = u.Boss;
        DATA.users.set(v.uname, u);

        const tr = document.createElement("tr");
        tr.setAttribute("data-uname", v.uname);
        tr.appendChild(UTIL.text_td(v.uname));
        tr.appendChild(UTIL.text_td(v.email));
        tr.appendChild(make_user_edit_button_td(v.uname, edit_boss));

        DISPLAY.boss_tbody.appendChild(tr);

    } else if(u.Teacher) {
        const v = u.Teacher.base;
        DATA.users.set(v.uname, u);

        const tr = document.createElement("tr");
        tr.setAttribute("data-uname", v.uname);
        tr.appendChild(UTIL.text_td(v.uname));
        tr.appendChild(UTIL.text_td(v.email));
        tr.appendChild(UTIL.text_td(u.Teacher.name));
        tr.appendChild(make_user_edit_button_td(v.uname, edit_teacher));

        DISPLAY.teacher_tbody.appendChild(tr);
    
    } else if(u.Student) {
        const v = u.Student.base;
        const s = u.Student;
        DATA.users.set(v.uname, u);

        const tr = document.createElement("tr");
        tr.setAttribute("data-uname", v.uname);
        tr.appendChild(UTIL.text_td(v.uname));
        tr.appendChild(UTIL.text_td(`${s.last}, ${s.rest}`));
        tr.appendChild(UTIL.text_td(s.teacher));
        tr.appendChild(UTIL.text_td(v.email));
        tr.appendChild(UTIL.text_td(s.parent));
        tr.appendChild(make_user_edit_button_td(v.uname, edit_student));

        DISPLAY.student_tbody.appendChild(tr);

    } else {
        console.log("add_user_to_display() not implemented for", u);
    }
}

/*

MAKING ACTUAL CHANGES SECTION

*/

/*
For editing current or adding new Admins.

If editing a current Admin, the `uname` input will be disabled.
This both prevents the uname from being changed (unames should
never be changed) and also signals the difference between adding
new and updating existing users.
*/
function edit_admin(evt) {
    const uname = this.getAttribute("data-uname");
    const form = document.forms['alter-admin'];
    const del = document.getElementById("delete-admin")
    del.setAttribute("data-uname", uname);

    if(uname) {
        const u = DATA.users.get(uname)['Admin'];
        form.elements['uname'].value = u.uname;
        form.elements['uname'].disabled = true;
        form.elements['email'].value = u.email;
        del.disabled = false
    } else {
        form.elements['uname'].disabled = false;
        for(const ipt of form.elements) {
            ipt.value = "";
        }
        del.disabled = true;
    }

    DISPLAY.admin_edit.showModal();
}

// We add this functionality to the "add Admin" button.
document.getElementById("add-admin").addEventListener("click", edit_admin);

/*
Performs some cursory validation and submits the updated Admin info to
the server.

Requests either "update-user" or "add-user" depending on whether the
`uname` input in the `alter-admin` form is diabled or not.

Will throw an error and prevent the dialog from closing if
form data pseudovalidation fails.
*/
function edit_admin_submit() {
    const form = document.forms['alter-admin'];
    const data = new FormData(form);
    /*  The FormData() constructor skips disabled inputs, so we need to
        manually ensure the `uname` value is in there. */
    const uname_input = form.elements['uname'];
    data.set("uname", uname_input.value);

    const uname = data.get("uname") || "";
    let email = data.get("email") || "";
    email = email.trim();

    const u = {
        "Admin": {
            "uname": uname,
            "email": email,
            "role": "Admin",
            "salt": "",
        }
    };

    DISPLAY.admin_edit.close();
    if(uname_input.disabled) {
        request_action("update-user", u, `Updating user ${uname}...`);
    } else {
        request_action("add-user", u, `Adding user ${uname}...`);
    }
}

// The "cancel" <button> should close the dialog but not try to submit the form.
document.getElementById("alter-admin-cancel")
    .addEventListener("click", (evt) => {
        evt.preventDefault();
        DISPLAY.admin_edit.close();
    });
document.getElementById("alter-admin-confirm")
    .addEventListener("click", edit_admin_submit);

async function delete_admin_submit(evt) {
    const uname = this.getAttribute("data-uname");
    const q = `Are you sure you want to delete Admin ${uname}?`;
    if(await are_you_sure(q)) {
        DISPLAY.admin_edit.close();
        request_action("delete-user", uname, `Deleting ${uname}...`);
    }
}

document.getElementById("delete-admin")
    .addEventListener("click", delete_admin_submit)


/*
For editing current or adding new Bosses.

(Much of what follows is essential identical to the above section
on adding/altering Admins.)

If editing a current Boss, the `uname` input will be disabled.
This both prevents the uname from being changed (unames should
never be changed) and also signals the difference between adding
new and updating existing users.
*/
function edit_boss(evt) {
    const uname = this.getAttribute("data-uname");
    const form = document.forms['alter-boss'];
    const del = document.getElementById("delete-boss")
    del.setAttribute("data-uname", uname);

    if(uname) {
        const u = DATA.users.get(uname)['Boss'];
        form.elements['uname'].value = u.uname;
        form.elements['uname'].disabled = true;
        form.elements['email'].value = u.email;
        del.disabled = false;
    } else {
        form.elements['uname'].disabled = false;
        for(const ipt of form.elements) {
            ipt.value = "";
        }
        del.removeAttribute("data-uname");
        del.disabled = true;
    }

    DISPLAY.boss_edit.showModal();
}

// We add this functionality to the "add Admin" button.
document.getElementById("add-boss").addEventListener("click", edit_boss);

/*
Performs some cursory validation and submits the updated Admin info to
the server.

Requests either "update-user" or "add-user" depending on whether the
`uname` input in the `alter-admin` form is diabled or not.

Will throw an error and prevent the dialog from closing if
form data pseudovalidation fails.
*/
function edit_boss_submit() {
    const form = document.forms['alter-boss'];
    const data = new FormData(form);
    /*  The FormData() constructor skips disabled inputs, so we need to
        manually ensure the `uname` value is in there. */
    const uname_input = form.elements['uname'];
    data.set("uname", uname_input.value);

    const uname = data.get("uname") || "";
    let email = data.get("email") || "";
    email = email.trim();

    const u = {
        "Boss": {
            "uname": uname,
            "email": email,
            "role": "Boss",
            "salt": "",
        }
    };

    DISPLAY.boss_edit.close();
    if(uname_input.disabled) {
        request_action("update-user", u, `Updating user ${uname}...`);
    } else {
        request_action("add-user", u, `Adding user ${uname}...`);
    }
}

// The "cancel" <button> should close the dialog but not try to submit the form.
document.getElementById("alter-boss-cancel")
    .addEventListener("click", (evt) => {
        evt.preventDefault();
        DISPLAY.boss_edit.close();
    });
document.getElementById("alter-boss-confirm")
    .addEventListener("click", edit_boss_submit);

async function delete_boss_submit(evt) {
    const uname = this.getAttribute("data-uname");
    const q = `Are you sure you want to delete Boss ${uname}?`;
    if(await are_you_sure(q)) {
        DISPLAY.boss_edit.close();
        request_action("delete-user", uname, `Deleting ${uname}...`);
    }
}

document.getElementById("delete-boss")
    .addEventListener("click", delete_boss_submit);


function edit_teacher(evt) {
    const uname = this.getAttribute("data-uname");
    const form = document.forms['alter-teacher'];
    const del = document.getElementById("delete-teacher");
    del.setAttribute("data-uname", uname);
    
    if(uname) {
        const u = DATA.users.get(uname)['Teacher'];
        form.elements['uname'].value = u.base.uname;
        form.elements['uname'].disabled = true;
        form.elements['email'].value = u.base.email;
        form.elements['name'].value = u.name;
        del.disabled = false;
    } else {
        for(const ipt of form.elements) {
            ipt.value = "";
        }
        del.removeAttribute("data-uname");
        del.disabled = true;
    }

    DISPLAY.teacher_edit.showModal();
}

document.getElementById("add-teacher")
    .addEventListener("click", edit_teacher);

function edit_teacher_submit() {
    const form = document.forms['alter-teacher'];
    const data = new FormData(form);
    // Manually ensure possibly-disabled input value still here.
    const uname_input = form.elements["uname"];
    data.set("uname", uname_input.value);

    const uname = data.get("uname") || "";
    const email = (data.get("email") || "").trim();
    const name = (data.get("name") || "").trim();

    const u = {
        "Teacher": {
            "base": {
                "uname": uname,
                "role": "Teacher",
                "salt": "",
                "email": email,
            },
            "name": name
        }
    };

    DISPLAY.teacher_edit.close();
    if(uname_input.disabled) {
        request_action("update-user", u, `Updating user ${uname}...`);
    } else {
        request_action("add-user", u, `Adding user ${uname}...`);
    }
}

document.getElementById("alter-teacher-cancel")
    .addEventListener("click", (evt => {
        evt.preventDefault();
        DISPLAY.teacher_edit.close();
    }));
document.getElementById("alter-teacher-confirm")
    .addEventListener("click", edit_teacher_submit);

async function delete_teacher_submit(evt) {
    const uname = this.getAttribute("data-uname");
    const q = `Are you sure you want to delete Teacher ${uname}?`;
    if(await are_you_sure(q)) {
        DISPLAY.teacher_edit.close();
        request_action("delete-user", uname, `Deleting ${uname}...`);
    }
}

document.getElementById("delete-teacher")
    .addEventListener("click", delete_teacher_submit);

function populate_teacher_selector(teacher_uname) {
    let sel = document.getElementById("alter-student-teacher");
    
    UTIL.clear(sel);

    for(const [uname, u] of DATA.users) {
        if(u.Teacher) {
            const opt = document.createElement("option");
            UTIL.set_text(opt, u.Teacher.name);
            opt.value = uname;
            sel.appendChild(opt);
        }
    }

    if(teacher_uname == "" || Boolean(teacher_uname)) {
        const null_opt = document.createElement("option");
        UTIL.set_text(null_opt, "[ None ]");
        null_opt.value = "";
        sel.appendChild(null_opt)
        sel.value = teacher_uname;
    }
}

function display_completion_history(uname) {
    const tbody = document.getElementById("alter-student-completion-history");
    UTIL.clear(tbody);
    for(const ipt of document.querySelectorAll("tbody#add-completion-history input")) {
        ipt.value = "";
    }
    UTIL.clear(document.getElementById("add-completion-spring-year"));

    const completions = DATA.completion.get(uname);
    if(completions) {
        for(const comp of completions) {
            const crs = DATA.courses.get(comp.sym);
            const tr = document.createElement("tr");

            let td = document.createElement("td");
            td.setAttribute("title", crs.book);
            UTIL.set_text(td, comp.sym)
            tr.appendChild(td);

            td = document.createElement("td");
            td.setAttribute("title", crs.book);
            UTIL.set_text(td, crs.title);
            tr.appendChild(td);

            td = document.createElement("td");
            UTIL.set_text(td, comp.term);
            tr.appendChild(td);

            td = document.createElement("td");
            const academic_year = `${comp.year}–${comp.year+1}`;
            UTIL.set_text(td, academic_year);
            tr.appendChild(td);

            td = document.createElement("td");
            const butt = document.createElement("button");
            butt.setAttribute("data-sym", comp.sym);
            butt.setAttribute("data-uname", uname);
            UTIL.label("🗙", butt);
            butt.setAttribute("title", `remove ${crs.title}`);
            butt.addEventListener("click", delete_completion);
            td.appendChild(butt);
            tr.appendChild(td);

            tbody.appendChild(tr);
        }
    }

    document.getElementById("add-completion-history-add")
        .setAttribute("data-uname", uname);
 
}

function edit_student(evt) {
    const uname = this.getAttribute("data-uname");
    const form = document.forms["alter-student"];
    const del = document.getElementById("delete-student");
    del.setAttribute("data-uname", uname);
    
    if(uname) {
        const u = DATA.users.get(uname)['Student'];
        const b = u.base;

        form.elements["uname"].value = b.uname;
        form.elements["uname"].disabled = true;
        form.elements["last"].value = u.last;
        form.elements["rest"].value = u.rest;
        form.elements["email"].value = b.email;
        form.elements["parent"].value = u.parent;
        populate_teacher_selector(u.teacher);
        del.disabled = false;

    } else {
        form.elements["uname"].disabled = false;
        for(const ipt of form.elements) {
            ipt.value = "";
        }
        populate_teacher_selector(null);
        del.removeAttribute("data-uname");
        del.disabled = true;
    }

    display_completion_history(uname);

    DISPLAY.student_edit.showModal();
}

document.querySelector("tbody#add-completion-history input[name='year']")
    .addEventListener("input", function(evt) {
        const span = document.getElementById("add-completion-spring-year");
        const year = Number(DISPLAY.history_year.value);
        let text = "";
        if(year) { text = `–${year + 1}`; }
        UTIL.set_text(span, text);
    });

document.getElementById("add-student")
    .addEventListener("click", edit_student);

function edit_student_submit() {
    const form = document.forms['alter-student'];
    const data = new FormData(form);
    const uname_input = form.elements['uname'];
    data.set("uname", uname_input.value);

    const uname = data.get("uname") || "";
    const email = (data.get("email") || "").trim();
    const last = data.get("last") || "";
    const rest = data.get("rest") || "";
    const teacher = data.get("teacher");
    const parent = (data.get("parent") || "").trim();

    let u = {
        "Student": {
            "base": {
                "uname": uname,
                "role": "Student",
                "salt": "",
                "email": email,
            },
            "last": last,
            "rest": rest,
            "teacher": teacher,
            "parent": parent,
            "fall_exam_fraction": 0.2,
            "spring_exam_fraction": 0.2,
            "fall_notices": 0,
            "spring_notices": 0,
        }
    };

    console.log("Inserting student:", u);

    DISPLAY.student_edit.close();
    if(uname_input.disabled) {
        request_action("update-user", u, `Updating user $[uname]...`);
    } else {
        request_action("add-user", u, `Adding user ${uname}...`);
    }
}

document.getElementById("alter-student-cancel")
    .addEventListener("click", (evt)=> {
        evt.preventDefault();
        DISPLAY.student_edit.close();
    });
document.getElementById("alter-student-confirm")
    .addEventListener("click", edit_student_submit);

async function delete_student_submit(evt) {
    const uname = this.getAttribute("data-uname");
    const q = `Are you sure you want to delete Student ${uname}?`;
    if(await are_you_sure(q)) {
        DISPLAY.student_edit.close();
        request_action("delete-user", uname, `Deleting ${uname}...`);
    }
}

document.getElementById("delete-student")
    .addEventListener("click", delete_student_submit);


document.getElementById("upload-students")
    .addEventListener("click", () => {
        DISPLAY.student_upload.showModal();
    });

function upload_students_submit(evt) {
    const form = document.forms["upload-students"];
    const data = new FormData(form);
    const file = data.get("file");

    UTIL.get_file_as_text(file)
    .then((text) => {
        DISPLAY.student_upload.close();
        request_action("upload-students", text, `Uploading new students...`);
    })
    .catch((err) => {
        RQ.add_err(`Error opening local file: ${err}`);
    })
}

document.getElementById("upload-students-confirm")
    .addEventListener("click",upload_students_submit);


/*

COURSES section

*/

function edit_course(evt) {
    const sym = this.getAttribute("data-sym");
    const form = document.forms['alter-course'];
    const del = document.getElementById("delete-course");

    if(sym) {
        const c = DATA.courses.get(sym);
        form.elements['sym'].value = c.sym;
        form.elements['sym'].disabled = true;
        form.elements['sym'].required = false;
        form.elements['title'].value = c.title;
        form.elements['level'].value = c.level;
        form.elements['book'].value = c.book || "";
        del.setAttribute("data-sym", sym);
        del.disabled = false;
    } else {
        for(const ipt of form.elements) {
            ipt.value = "";
        }
        form.elements["sym"].disabled = false;
        form.elements["sym"].required = true;
        del.removeAttribute("data-sym");
        del.disabled = true;
    }

    DISPLAY.course_edit.showModal();
}

document.getElementById("add-course")
    .addEventListener("click", edit_course);

function edit_course_submit() {
    const form = document.forms['alter-course'];
    const data = new FormData(form);
    // Manually ensure possibly-disabled input value.
    const sym_input = form.elements['sym'];
    data.set("sym", sym_input.value);

    const sym = data.get("sym");
    const title = (data.get("title") || "").trim();
    const level = Number(data.get("level"));
    if(!level) {
        RQ.add_err("Course level must be a decimal number reflecting its position in the grade-level sequence.");
        return;
    }
    let book = data.get("book").trim();
    if(book == "") { book = null; }

    let c = DATA.courses.get(sym);
    if(c) {
        // If .get()ting from DATA.courses returns an object, that means
        // we are altering an extant course. We update the old course's values
        // to those from the form, leaving the other values intact.
        c.title = title;
        c.level = level;
        c.book = book;
        // This function doesn't alter any chapters, so we empty this and
        // send less unnecessary data to the server.
        c.chapters = [];
    } else {
        // .get()ting from DATA.courses returned nothing, which means we are
        // creating a new course. We build the course object to send to
        // the server.
        c = {
            // This value doesn't matter, as it will be set for us when
            // it's inserted into the database.
            "id": 0,
            "sym": sym,
            "title": title,
            "level": level,
            "book": book,
            "chapters": [], // No chapters yet!
        };
    }

    DISPLAY.course_edit.close();
    if(sym_input.disabled) {
        request_action("update-course", c, `Updating course ${sym} (${title}).`);
    } else {
        request_action("add-course", c, `Adding new course ${sym} (${title}).`);
    }
}

document.getElementById("alter-course-cancel")
    .addEventListener("click", (evt) => {
        evt.preventDefault();
        DISPLAY.course_edit.close();
    });
document.getElementById("alter-course-confirm")
    .addEventListener("click", edit_course_submit);

async function delete_course_submit(evt) {
    const sym = this.getAttribute("data-sym");
    const c = DATA.courses.get(sym);
    const q = `Are you sure you want to delete Course ${sym} (${c.title})?`;
    if(await are_you_sure(q)) {
        DISPLAY.course_edit.close();
        request_action("delete-course", sym, `Deleting Course ${sym} (${c.title}).`);
    }
}

document.getElementById("delete-course")
    .addEventListener("click", delete_course_submit);

function append_chapter(evt) {
    const sym = this.getAttribute("data-sym");
    const c = DATA.courses.get(sym);
    
    let new_ch_n = 1;
    if(c.chapters.at(-1)) {
        new_ch_n = c.chapters.at(-1).seq + 1;
    }

    const new_ch = {
        // Will be set appropriately on insertion into database.
        "id": 0,
        "course_id": c.id,
        "seq": new_ch_n,
        "title": `Chapter ${new_ch_n}`,
        "subject": null,
        "weight": 1.0,
    };

    const chapters = [new_ch];

    request_action("add-chapters", chapters, `Adding Chapter ${new_ch_n} to ${sym} (${c.title}).`);
}

function append_n_chapters(evt) {
    evt.preventDefault();

    const sym = this.getAttribute("data-sym");
    const c = DATA.courses.get(sym);
    const form_name = this.getAttribute("data-form-name");
    const form = document.forms[form_name];
    const data = new FormData(form);
    const n_chs = Number(data.get("number"));

    let first_ch_n = 1;
    if(c.chapters.at(-1)) {
        first_ch_n = c.chapters.at(-1).seq + 1;
    }

    const chapters = new Array();

    for(let n = 0; n < n_chs; n++) {
        const ch_n = n + first_ch_n;

        const new_ch = {
            // Will be set appropriately upon insertion into database.
            "id": 0,
            "course_id": c.id,
            "seq": ch_n,
            "title": `Chapter ${ch_n}`,
            "subject": null,
            "weight": 1.0,
        };

        chapters.push(new_ch);
    }

    request_action("add-chapters", chapters, `Adding ${chapters.length} Chapters to ${sym} (${c.title})`);
}

function edit_chapter(evt) {
    const sym = this.getAttribute("data-sym");
    const idx = Number(this.getAttribute("data-index"));
    const ch = DATA.courses.get(sym).chapters[idx];
    const form = document.forms["alter-chapter"];
    const del = document.getElementById("delete-chapter");
    del.setAttribute("data-id", ch.id);
    del.setAttribute("data-description", `${ch.title}`);

    form.elements["id"].value = ch.id;
    form.elements["seq"].value = ch.seq;
    form.elements["title"].value = ch.title;
    form.elements["subject"].value = ch.subject;
    form.elements["weight"].value = ch.weight;

    DISPLAY.chapter_edit.showModal();
}

function edit_chapter_submit(evt) {
    const form = document.forms["alter-chapter"];
    const data = new FormData(form);

    const ch = {
        "id": Number(data.get("id")),
        // The server won't change this, so it doesn't matter.
        "course_id": 0,
        "seq": Number(data.get("seq")),
        "title": data.get("title").trim(),
        "subject": data.get("subject").trim(),
        "weight": (Number(data.get("weight")) || 1.0)
    };

    DISPLAY.chapter_edit.close();
    request_action("update-chapter", ch, `Updating chapter details.`);
}
document.getElementById("alter-chapter-cancel")
    .addEventListener("click", (evt) => {
        evt.preventDefault();
        DISPLAY.chapter_edit.close();
    });
document.getElementById("alter-chapter-confirm")
    .addEventListener("click", edit_chapter_submit);

async function delete_chapter_submit(evt) {
    const id = this.getAttribute("data-id");
    const desc = this.getAttribute("data-description");
    const q = `Are you sure you want to delete ${desc} from this Course?`;
    if(await are_you_sure(q)) {
        DISPLAY.chapter_edit.close();
        request_action("delete-chapter", id, `Deleting Chapter "${desc}.`);
    }
}

document.getElementById("delete-chapter")
    .addEventListener("click", delete_chapter_submit);

function toggle_chapter_display(evt) {
    const sym = this.getAttribute("data-sym");
    let tr = document.querySelector(`tr[data-chapters="${sym}"]`);
    if(tr.style.display == "table-row") {
        tr.style.display = "none";
        UTIL.set_text(this, "\u2304");
        this.setAttribute("title", "show chapter list");
    } else {
        tr.style.display = "table-row";
        UTIL.set_text(this, "\u2303");
        this.setAttribute("title", "hide chapter list");
    }
}

function populate_course_chapters(c) {
    const sym = c.sym;
    const td_container = document.querySelector(`tr[data-chapters="${sym}"] > td`);
    UTIL.clear(td_container);

    const tab = document.createElement("table");
    tab.id = `${sym}-chapters`;
    tab.setAttribute("class", "chapter-table");

    const thead = document.createElement("thead");
    const tr = document.createElement("tr");
    tr.appendChild(UTIL.text_th("#"));
    tr.appendChild(UTIL.text_th("title"));
    tr.appendChild(UTIL.text_th("subject"));
    tr.appendChild(UTIL.text_th("weight"));
    tr.appendChild(UTIL.text_th("actions"));
    thead.appendChild(tr);
    tab.appendChild(thead);

    const tbody = document.createElement("tbody");
    c.chapters.forEach((ch, n) => {
        const tr = document.createElement("tr");
        tr.setAttribute("data-id", ch.id);
        tr.appendChild(UTIL.text_td(ch.seq));
        tr.appendChild(UTIL.text_td(ch.title));
        tr.appendChild(UTIL.text_td(ch.subject || ""));
        tr.appendChild(UTIL.text_td(ch.weight));
        const td = document.createElement("td");
        const ebutt = document.createElement("button");
        UTIL.label("edit", ebutt);
        ebutt.setAttribute("data-sym", sym);
        ebutt.setAttribute("data-index", n);
        ebutt.addEventListener("click", edit_chapter);
        td.appendChild(ebutt);
        tr.appendChild(td);
        tbody.appendChild(tr);
    });
    tab.appendChild(tbody);

    td_container.appendChild(tab);

    const div = document.createElement("div");
    div.setAttribute("class", "chapter-append");

    const add_butt = document.createElement("button");
    add_butt.setAttribute("data-sym", sym);
    UTIL.label("+ append Chapter", add_butt);
    add_butt.addEventListener("click", append_chapter)
    div.appendChild(add_butt);

    const form_name = `append-${sym}`;
    const form = document.createElement("form");
    form.setAttribute("name", form_name);
    form.setAttribute("class", "append-form");

    const n_ch_ipt = document.createElement("input");
    n_ch_ipt.setAttribute("type", "number");
    n_ch_ipt.setAttribute("name", "number");
    n_ch_ipt.setAttribute("min", "1");
    n_ch_ipt.setAttribute("max", "64");
    n_ch_ipt.required = true;
    form.appendChild(n_ch_ipt);

    const app_butt = document.createElement("button");
    app_butt.setAttribute("data-form-name", form_name);
    app_butt.setAttribute("data-sym", sym);
    UTIL.label("+ append N Chapters", app_butt);
    app_butt.addEventListener("click", append_n_chapters);
    form.appendChild(app_butt);

    div.appendChild(form);

    td_container.appendChild(div);
}

function populate_course_table_row(c) {
    const tr = DISPLAY.course_tbody.querySelector(`tr[data-sym="${c.sym}"]`);
    UTIL.clear(tr);

    tr.appendChild(UTIL.text_td(c.sym));
    tr.appendChild(UTIL.text_td(c.title));
    tr.appendChild(UTIL.text_td(c.level));
    let td = document.createElement("td");
    const cite = document.createElement("cite");
    UTIL.set_text(cite, c.book);
    td.appendChild(cite);
    tr.appendChild(td);
    tr.appendChild(UTIL.text_td(c.chapters.length));

    td = document.createElement("td");
    
    const expand = document.createElement("button");
    expand.setAttribute("data-sym", c.sym);
    UTIL.set_text(expand, "\u2304");
    expand.setAttribute("title", "show chapter list");
    expand.addEventListener("click", toggle_chapter_display);
    td.appendChild(expand);

    const ebutt = document.createElement("button");
    ebutt.setAttribute("data-sym", c.sym);
    UTIL.label("edit", ebutt);
    ebutt.addEventListener("click", edit_course);
    td.appendChild(ebutt);

    tr.appendChild(td);
}

function populate_courses(r) {
    r.json()
    .then(j => {
        console.log("populate-courses response:", j);

        DATA.courses = new Map();
        UTIL.clear(DISPLAY.course_tbody);
        const list = document.getElementById("course-names");
        UTIL.clear(list);

        for(const c of j) {
            DATA.courses.set(c.sym, c);

            // Create and populate <TR> element to hold course metadata.
            let tr = document.createElement("tr");
            tr.setAttribute("data-sym", c.sym);
            DISPLAY.course_tbody.appendChild(tr);
            populate_course_table_row(c);

            // Create and populate <TR> (and nested single <TD>)
            // to hold chapter table.
            tr = document.createElement("tr");
            tr.setAttribute("data-chapters", c.sym);
            const td = document.createElement("td");
            td.setAttribute("colspan", "6");
            tr.appendChild(td);
            DISPLAY.course_tbody.appendChild(tr);
            populate_course_chapters(c);

            // Add an <OPTION> to the course names <DATALIST>
            let book_text = "";
            if(c.book) { book_text = ` (${c.book})`; }
            const opt_text = `${c.sym}: ${c.title}${book_text}`;
            const opt = document.createElement("option");
            opt.value = c.sym;
            UTIL.set_text(opt, opt_text);
            list.appendChild(opt);
        }
    }).catch(RQ.add_err);
}

document.getElementById("upload-course")
    .addEventListener("click", () => {
        DISPLAY.course_upload.showModal();
    });

function upload_course_submit(evt) {
    const form = document.forms["upload-course"];
    const data = new FormData(form);
    const file = data.get("file");

    UTIL.get_file_as_text(file)
    .then((text) => {
        DISPLAY.course_upload.close();
        request_action("upload-course", text, `Uploading new students...`);
    })
    .catch((err) => {
        RQ.add_err(`Error opening local file: ${err}`);
    });
}

document.getElementById("upload-course-confirm")
    .addEventListener("click", upload_course_submit);

document.getElementById("reset-students-button")
    .addEventListener("click", () => {
        DISPLAY.student_reset.showModal();
    });
document.getElementById("reset-students-cancel")
    .addEventListener("click", (evt) => {
        evt.preventDefault();
        DISPLAY.student_reset.close();
    });
document.getElementById("reset-students-confirm")
    .addEventListener("click", async function(evt) {
        evt.preventDefault();
        const q = "Are you sure you want to exercise the scorced-earth nuclear option on all student Goals and Student User records?";
        if(await are_you_sure(q)) {
            DISPLAY.student_reset.close();
            request_action("reset-students", null, "Deleting all student data.")
        }
    });

function add_completion(evt) {
    evt.preventDefault();

    const uname = this.getAttribute("data-uname");
    const subbod = document.getElementById("add-completion-history");
    const sym = subbod.querySelector("input[name='course']").value.trim();
    const term = subbod.querySelector("select").value;
    let year = subbod.querySelector("input[name='year']").value.trim();
    year = Number(year);
    if(!year) {
        RQ.add_err("You should enter a valid year.");
        return;
    }
    if(year < 2001) {
        RQ.add_err("The year should probably be some time this millennium.");
        return;
    }
    if(!sym) {
        RQ.add_err("You should enter a course symbol.");
        return;
    }

    const extra_headers = { "x-camp-student": uname };
    const body = {
        "sym": sym,
        "year": year,
        "term": term,
    };
    const stud = DATA.users.get(uname);
    const desc = `Adding completed course to history for ${stud.rest} ${stud.last}.`;

    request_action("add-completion", body, desc, extra_headers);
}

document.getElementById("add-completion-history-add")
    .addEventListener("click", add_completion);

function delete_completion(evt) {
    evt.preventDefault();

    const uname = this.getAttribute("data-uname");
    const sym = this.getAttribute("data-sym");

    const extra_headers = {
        "x-camp-student": uname,
        "x-camp-course": sym,
    };
    const stud = DATA.users.get(uname);
    const desc = `Deleting course "${sym}" from course history for ${stud.rest} ${stud.last}.`;

    request_action("delete-completion", null, desc, extra_headers);
}

/*

PAGE LOAD SECTION

*/

console.log(DISPLAY);

UTIL.ensure_on_load(() => {
    request_action("populate-users", "", "Fetching User data...");
    request_action("populate-completion", "", "Fetching Course completion history...");
    request_action("populate-courses", "", "Fetching Course data...");
});
