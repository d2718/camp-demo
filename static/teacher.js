/*
JS BS for Teacher interaction.
*/
"use strict";

const API_ENDPOINT = "/teacher";
const DATA = {
    courses: new Map(),
    chapters: new Map(),
    paces: new Map(),
    goals: new Map(),
    traits: [],
};
const DISPLAY = {
    course_list_div: document.getElementById("course-info"),
    course_list_hide: document.getElementById("course-info-hide"),
    course_list_genl: document.querySelector("table#genl-courses > tbody"),
    course_list_hs: document.querySelector("table#hs-courses > tbody"),
    calbox: document.getElementById("cals"),
    upload_goals: document.getElementById("upload-goals-dialog"),
    goal_edit: document.getElementById("edit-goal"),
    goal_edit_meta: document.getElementById("edit-goal-meta"),
    course_input: document.getElementById("edit-goal-course"),
    seq_input: document.getElementById("edit-goal-seq"),
    goal_complete: document.getElementById("complete-goal"),
    goal_complete_meta: document.getElementById("complete-goal-meta"),
    sidecar_edit: document.getElementById("edit-sidecar"),
    report_edit: document.getElementById("edit-report"),
    pdf_view: document.getElementById("view-pdf"),
};
const GOAL_MASTERY_OPTS = [
    {val: "Not", text: "Not Mastered"},
    {val: "Mastered", text: "Mastered"},
    {val: "Retained", text: "Mastered & Retained"}
];

const NOW = new Date();

let next_err = function() {}
{
    let err_count = 0;
    next_err = function() {
        const err = err_count;
        err_count += 1;
        return err;
    }
}

function log_numbered_error(e) {
    const errno = next_err();
    const err_txt = `${e} (See console error #${errno}.)`;
    console.error(`Error #${errno}:`, e, e.stack);
    RQ.add_err(err_txt);
}

function ratio2pct(num, denom) {
    if(Math.abs(denom) < 0.0001) { return "0%"; }
    const pct = Math.round(100 * num / denom);
    return `${pct}%`;
}

function interpret_score(str) {
    const [n, d] = str.split("/").map(x => Number(x));
    if(!n) {
        return null;
    } else if(d) {
        return n / d;
    } else if(n > 2) {
        return n / 100;
    } else {
        return n;
    }
}
function score2pct(str) {
    const p = interpret_score(str);
    if(p) {
        const pct = Math.round(100 * p);
        return `${pct}%`;
    }
}

function input_label_pair(label, id, name, type) {
    const input = document.createElement("input");
    if(type) { input.setAttribute("type", type); }
    input.setAttribute("name", name);
    input.id = id;
    const lab = document.createElement("label");
    lab.setAttribute("for", id);
    UTIL.set_text(lab, label);
    return [input, lab];
}

const PCAL_COLS = ["course", "chapter", "due", "done", "tries", "score", "edit"];

function row_from_goal(g) {
    const crs = DATA.courses.get(g.sym);
    const chp = DATA.chapters.get(crs.chapters[g.seq]);

    const tr = document.createElement("tr");
    tr.setAttribute("data-id", g.id);
    let due = null;
    let done = null;
    if(g.due) { due = UTIL.iso2date(g.due); }
    if(g.done) { done = UTIL.iso2date(g.done); }
    if(due) {
        if(done) {
            if(due < done) {
                tr.setAttribute("class", "late");
            } else {
                tr.setAttribute("class", "done");
            }
        } else {
            if(due < NOW) {
                tr.setAttribute("class", "due");
            } else {
                tr.setAttribute("class", "yet");
            }
            // If it's not done, and also "Incomplete", it should be
            // highlighted with badness.
            if(g.inc) { tr.classList.add("bad"); }
        }
    } else {
        if(done) {
            tr.setAttribute("class", "done");
        } else {
            tr.setAttribute("class", "yet");
        }
    }

    const ctd = UTIL.text_td(crs.title);
    ctd.setAttribute("title", crs.book);
    tr.appendChild(ctd);

    let chtext = chp.title;
    if(g.rev) { chtext = chtext + " R"; }
    if(g.inc) { chtext = chtext + " I"; }
    const chtd = UTIL.text_td(chtext)
    if(chp.subject) { chtd.setAttribute("title", chp.subject); }
    tr.appendChild(chtd);

    const duetd = UTIL.text_td(g.due || "")
    duetd.setAttribute("class", "due");
    tr.appendChild(duetd);
    const donetd = UTIL.text_td(g.done || "");
    donetd.setAttribute("class", "done");
    tr.appendChild(donetd);
    const triestd = UTIL.text_td(g.tries || "")
    triestd.setAttribute("class", "tries");
    tr.appendChild(triestd);
    const scoretd = document.createElement("td");
    if(g.score) {
        UTIL.set_text(scoretd, `${g.score} (${score2pct(g.score)})`);
    }
    scoretd.setAttribute("class", "score");
    tr.appendChild(scoretd);

    const etd = document.createElement("td");
    etd.setAttribute("class", "edit");
    const complete = document.createElement("button");
    complete.setAttribute("data-id", g.id);
    complete.setAttribute("title", "complete goal");
    UTIL.label("\u2713", complete);
    complete.addEventListener("click", complete_goal);
    etd.appendChild(complete);
    const edit = document.createElement("button");
    edit.setAttribute("data-id", g.id);
    edit.setAttribute("title", "edit goal");
    UTIL.label("\u270e", edit);
    edit.addEventListener("click", edit_goal);
    etd.appendChild(edit);
    tr.appendChild(etd);

    return tr;
}

function toggle_extra(evt) {
    const uname = this.getAttribute("data-uname");
    const tab = document.querySelector(`table.pace[data-uname="${uname}"]`);
    const extra = tab.querySelector("tr.extra");
    const butt = tab.querySelector("button.expander");
    const link = tab.querySelector("tr.more a");
    const lab = butt.querySelector("label");

    if(extra.style.display == "table-row") {
        extra.style.display = "none";
        link.style.display = "none";
        UTIL.set_text(lab, "\u2304 more \u2304");
    } else {
        extra.style.display = "table-row";
        link.style.display = "inline";
        UTIL.set_text(lab, "\u2303 less \u2303");
    }
}

function make_calendar_footer(cal) {
    const ex_tr = document.createElement("tr");
    ex_tr.setAttribute("class", "extra");
    const ex_td = document.createElement("td");
    ex_td.setAttribute("colspan", PCAL_COLS.length);
    const form = document.createElement("form");
    const form_id = `${cal.uname}-extra`;
    form.id = form_id;
    form.setAttribute("name", form_id);
    
    let ipt, lab;
    [ipt, lab] = input_label_pair("Fall Notices", `${cal.uname}-fall-notices`, "fall-notices", "number");
    form.appendChild(lab); form.appendChild(ipt);
    ipt.setAttribute("min", 0);
    ipt.required = true;
    ipt.value = cal.fnot ? cal.fnot : 0;
    [ipt, lab] = input_label_pair("Spring Notices", `${cal.uname}-spring-notices`, "spring-notices", "number");
    form.appendChild(ipt); form.appendChild(lab);
    ipt.setAttribute("min", 0);
    ipt.required = true;
    ipt.value = cal.snot ? cal.snot : 0;
    [ipt, lab] = input_label_pair("Fall Exam Score", `${cal.uname}-fall-exam`, "fall-exam");
    form.appendChild(lab); form.appendChild(ipt);
    if(cal.fex) { ipt.value = cal.fex; }
    [ipt, lab] = input_label_pair("Spring Exam Score", `${cal.uname}-spring-exam`, "spring-exam");
    form.appendChild(ipt); form.appendChild(lab);
    if(cal.sex) { ipt.value = cal.sex; }
    [ipt, lab] = input_label_pair("Fall Exam Fraction", `${cal.uname}-fall-exam-frac`, "fall-exam-frac", "number");
    ipt.setAttribute("min", "0.00");
    ipt.setAttribute("max", "1.00");
    ipt.setAttribute("step", "0.01");
    ipt.value = cal.fex_frac;
    ipt.required = true;
    form.appendChild(lab); form.appendChild(ipt);
    [ipt, lab] = input_label_pair("Spring Exam Fraction", `${cal.uname}-spring-exam-frac`, "spring-exam-frac", "number");
    ipt.setAttribute("min", "0.00");
    ipt.setAttribute("max", "1.00");
    ipt.setAttribute("step", "0.01");
    ipt.value = cal.sex_frac;
    ipt.required = true;
    form.appendChild(ipt), form.appendChild(lab);

    const exsub_butt = document.createElement("button");
    exsub_butt.setAttribute("data-uname", cal.uname);
    exsub_butt.setAttribute("data-formname", form_id);
    UTIL.label("update", exsub_butt);
    exsub_butt.addEventListener("click", update_numbers_submit);
    form.appendChild(exsub_butt);
    ex_td.appendChild(form);
    //ex_td.appendChild(document.createElement("br"));

    const last_div = document.createElement("div");
    const autobutt = document.createElement("button");
    UTIL.label("autopace", autobutt);
    autobutt.setAttribute("data-uname", cal.uname);
    autobutt.addEventListener("click", autopace);
    last_div.appendChild(autobutt);
    const sidecarbutt = document.createElement("button");
    UTIL.label("report info", sidecarbutt);
    sidecarbutt.setAttribute("data-uname", cal.uname);
    sidecarbutt.addEventListener("click", edit_sidecar);
    last_div.appendChild(sidecarbutt);
    const nuke = document.createElement("button");
    UTIL.label("clear all goals", nuke);
    nuke.setAttribute("data-uname", cal.uname);
    nuke.addEventListener("click", clear_goals);
    last_div.appendChild(nuke);
    ex_td.appendChild(last_div);

    ex_tr.appendChild(ex_td);

    return ex_tr;
}

function add_grades_to_calendar(tab, cal) {
    const sem_div = DATA.dates.get("end-fall");

    const semf_due = [];
    const sems_due = [];
    const semf_done = [];
    const sems_done = [];

    let semf_inc = false;
    let sems_inc = false;

    for(const g of cal.goals) {
        if(g.due) {
            const due = UTIL.iso2date(g.due);
            if(due < sem_div) {
                semf_due.push(g);
                if(!g.done) { semf_inc = true; }
            } else {
                sems_due.push(g);
                if(!g.done) { sems_inc = true; }
            }
        }
        if(g.done) {
            const done = UTIL.iso2date(g.done);
            if(done < sem_div) {
                semf_done.push(g);
            } else {
                sems_done.push(g);
            }
        }
    }

    console.debug(semf_due, sems_due, semf_done, sems_done, semf_inc, sems_inc);

    if(semf_done.length > 0) {
        const final_goal = semf_done.at(-1);
        const test_avg = semf_done.reduce((prev, cur) => {
            const score = interpret_score(cur.score);
            return prev + score;
        }, 0.0) / semf_done.length;
        let test_pct = test_avg * 100.0;

        const test_tr = document.createElement("tr");
        test_tr.setAttribute("class", "semsum");
        let td = UTIL.text_td("Fall Test Average:");
        td.setAttribute("colspan", "5");
        test_tr.appendChild(td);
        let test_text = `${Math.round(test_pct)}%`;
        if(semf_inc) { test_text = test_text + " (I)"; }
        td = UTIL.text_td(test_text);
        td.setAttribute("colspan", "2");
        test_tr.appendChild(td);

        const final_tr = tab.querySelector(`tr[data-id="${final_goal.id}"]`);
        final_tr.insertAdjacentElement("afterend", test_tr);

        if(cal.fex) {
            const exam_tr = document.createElement("tr");
            exam_tr.setAttribute("class", "semsum");
            let td = UTIL.text_td("Fall Final:");
            td.setAttribute("colspan", "5");
            exam_tr.appendChild(td);
            const exam_score = interpret_score(cal.fex) * 100;
            const exam_text = `${Math.round(exam_score)}%`;
            td = UTIL.text_td(exam_text);
            td.setAttribute("colspan", "2");
            exam_tr.appendChild(td);
            test_tr.insertAdjacentElement("afterend", exam_tr);
            
            let sem_grade = (exam_score * cal.fex_frac) + (test_pct * (1.0 - cal.fex_frac));

            let notice_tr = null;
            if(cal.fnot > 0) {
                notice_tr = document.createElement("tr");
                notice_tr.setAttribute("class", "semsum");
                let td = UTIL.text_td("Notices:");
                td.setAttribute("colspan", "5");
                notice_tr.appendChild(td);
                const notice_text = `-${cal.fnot}%`;
                td = UTIL.text_td(notice_text);
                td.setAttribute("colspan", "2");
                notice_tr.appendChild(td);
                exam_tr.insertAdjacentElement("afterend", notice_tr);
                sem_grade = sem_grade - cal.fnot;
            }

            const sem_tr = document.createElement("tr");
            sem_tr.setAttribute("class", "semsum");
            td = UTIL.text_td("Fall Semester Grade:");
            td.setAttribute("colspan", "5");
            sem_tr.appendChild(td);
            let sem_text = `${Math.round(sem_grade)}%`;
            if(semf_inc) { sem_text = sem_text + " (I)"; }
            td = UTIL.text_td(sem_text);
            td.setAttribute("colspan", "2");
            sem_tr.appendChild(td);

            if(notice_tr) {
                notice_tr.insertAdjacentElement("afterend", sem_tr);
            } else {
                exam_tr.insertAdjacentElement("afterend", sem_tr);
            }
        }
    }

    if(sems_done.length > 0) {
        const final_goal = sems_done.at(-1);
        const test_avg = sems_done.reduce((prev, cur) => {
            const score = interpret_score(cur.score);
            return prev + score;
        }, 0.0) / sems_done.length;
        let test_pct = test_avg * 100.0;

        const test_tr = document.createElement("tr");
        test_tr.setAttribute("class", "semsum");
        let td = UTIL.text_td("Spring Test Average:");
        td.setAttribute("colspan", "5");
        test_tr.appendChild(td);
        let test_text = `${Math.round(test_pct)}%`;
        if(semf_inc) { test_text = test_text + " (I)"; }
        td = UTIL.text_td(test_text);
        td.setAttribute("colspan", "2");
        test_tr.appendChild(td);

        const final_tr = tab.querySelector(`tr[data-id="${final_goal.id}"]`);
        final_tr.insertAdjacentElement("afterend", test_tr);

        if(cal.sex) {
            const exam_tr = document.createElement("tr");
            exam_tr.setAttribute("class", "semsum");
            let td = UTIL.text_td("Spring Final:");
            td.setAttribute("colspan", "5");
            exam_tr.appendChild(td);
            const exam_score = interpret_score(cal.sex) * 100;
            const exam_text = `${Math.round(exam_score)}%`;
            td = UTIL.text_td(exam_text);
            td.setAttribute("colspan", "2");
            exam_tr.appendChild(td);
            test_tr.insertAdjacentElement("afterend", exam_tr);
            
            let sem_grade = (exam_score * cal.sex_frac) + (test_pct * (1.0 - cal.sex_frac));

            let notice_tr = null;
            if(cal.snot > 0) {
                notice_tr = document.createElement("tr");
                notice_tr.setAttribute("class", "semsum");
                let td = UTIL.text_td("Notices:");
                td.setAttribute("colspan", "5");
                notice_tr.appendChild(td);
                const notice_text = `-${cal.snot}%`;
                td = UTIL.text_td(notice_text);
                td.setAttribute("colspan", "2");
                notice_tr.appendChild(td);
                exam_tr.insertAdjacentElement("afterend", notice_tr);
                sem_grade = sem_grade - cal.snot;
            }

            const sem_tr = document.createElement("tr");
            sem_tr.setAttribute("class", "semsum");
            td = UTIL.text_td("Spring Semester Grade:");
            td.setAttribute("colspan", "5");
            sem_tr.appendChild(td);
            let sem_text = `${Math.round(sem_grade)}%`;
            if(semf_inc) { sem_text = sem_text + " (I)"; }
            td = UTIL.text_td(sem_text);
            td.setAttribute("colspan", "2");
            sem_tr.appendChild(td);

            if(notice_tr) {
                notice_tr.insertAdjacentElement("afterend", sem_tr);
            } else {
                exam_tr.insertAdjacentElement("afterend", sem_tr);
            }
        }
    }
}

function make_calendar_table(cal) {
    const tab = document.createElement("table");
    tab.setAttribute("class", "pace");
    tab.setAttribute("data-uname", cal.uname);

    const thead = document.createElement("thead");
    tab.appendChild(thead);
    const sum_row = document.createElement("tr");
    const sum_td = document.createElement("td");
    sum_td.setAttribute("colspan", String(PCAL_COLS.length));
    const summary = document.createElement("div");
    summary.setAttribute("class", "summary");
    sum_td.appendChild(summary);
    sum_row.appendChild(sum_td);
    thead.appendChild(sum_row);
    {
        const tr = document.createElement("tr");
        for(const lab of PCAL_COLS) {
            tr.appendChild(UTIL.text_th(lab));
        }
        thead.appendChild(tr);
    }
    
    const tbody = document.createElement("tbody");
    tab.appendChild(tbody);

    let n_due = 0;
    let n_done = 0;

    for(const g of cal.goals) {
        tbody.appendChild(row_from_goal(g));
        if(g.done) {
            n_done += 1;
        }
        if(g.due) {
            let due = UTIL.iso2date(g.due);
            if(due < NOW) {
                n_due += 1;
            }
        }
    }

    // Populate table's <THEAD> with name and uname.
    const names = document.createElement("div");
    let name = document.createElement("span");
    name.setAttribute("class", "full");
    UTIL.set_text(name, `${cal.last}, ${cal.rest}`);
    names.appendChild(name);
    names.appendChild(document.createElement("br"));
    name = document.createElement("kbd");
    name.setAttribute("class", "uname");
    UTIL.set_text(name, cal.uname);
    names.appendChild(name);
    summary.appendChild(names);

    // Populate table's <THEAD> with #due/#done (pct).
    const numbers = document.createElement("div");
    let lead_pct = ratio2pct(cal.done_weight - cal.due_weight, cal.total_weight);
    if(cal.done_weight >= cal.due_weight) {
        lead_pct = "+" + lead_pct;
    } else {
        numbers.setAttribute("class", "bad");
    }
    const num_txt = `done ${n_done} / ${n_due} due (${lead_pct})`;
    UTIL.set_text(numbers, num_txt);
    summary.appendChild(numbers);

    // Create row with extras-expander button and add-goal button.
    const more_tr = document.createElement("tr");
    more_tr.setAttribute("class", "more");
    const more_td = document.createElement("td");
    more_tr.appendChild(more_td);
    more_td.setAttribute("colspan", PCAL_COLS.length);
    const more_div = document.createElement("div");
    more_div.setAttribute("class", "fullwidth");
    more_td.appendChild(more_div);

    const expbutt = document.createElement("button");
    expbutt.setAttribute("data-uname", cal.uname);
    expbutt.setAttribute("class", "expander");
    UTIL.label("\u2304 more \u2304", expbutt);
    expbutt.addEventListener("click", toggle_extra);
    more_div.appendChild(expbutt);
    const help_a = document.createElement("a");
    help_a.setAttribute("href", "/static/help/teacher.html#toc-footer");
    help_a.setAttribute("rel", "help");
    help_a.setAttribute("target", "_blank");
    help_a.innerHTML = "&#x1f6c8;";
    more_div.appendChild(help_a);
    const addbutt = document.createElement("button");
    addbutt.setAttribute("data-uname", cal.uname);
    UTIL.label("add goal \u229e", addbutt);
    addbutt.addEventListener("click", edit_goal);
    more_div.appendChild(addbutt);

    tbody.appendChild(more_tr);

    // Create extras row and populate.
    const ex_tr = make_calendar_footer(cal);
    tbody.appendChild(ex_tr);

    add_grades_to_calendar(tab, cal);

    return tab;
}

function populate_courses(r) {
    r.json()
    .then(j => {
        console.log("populate-courses response:", j);
        j.sort((a, b) => a.level - b.level);

        DATA.courses = new Map();
        DATA.chapters = new Map();
        const list = document.getElementById("course-names");
        UTIL.clear(list);
        UTIL.clear(DISPLAY.course_list_genl);
        UTIL.clear(DISPLAY.course_list_hs);

        for(const crs of j) {
            let chaps = new Array();
            for(const chp of crs.chapters) {
                DATA.chapters.set(chp.id, chp);
                chaps[chp.seq] = chp.id;
            }
            crs.chapters = chaps;
            DATA.courses.set(crs.sym, crs);

            let book_text = "";
            if(crs.book) { book_text = ` (${crs.book})`; }
            const option_text = `${crs.sym}: ${crs.title}${book_text}`;
            const opt = document.createElement("option");
            opt.value = crs.sym;
            UTIL.set_text(opt, option_text);
            list.appendChild(opt);

            const tr = document.createElement("tr");
            tr.appendChild(UTIL.text_td(crs.sym));
            const titletd = UTIL.text_td(crs.title);
            if(crs.book) {
                const cite = document.createElement("cite");
                UTIL.set_text(cite, crs.book);
                titletd.appendChild(cite);
            }
            tr.appendChild(titletd);
            if(crs.level < 9.0) {
                DISPLAY.course_list_genl.appendChild(tr);
            } else {
                DISPLAY.course_list_hs.appendChild(tr);
            }
        }

        request_action("populate-goals", "", "Populating pace calendars.")
    })
    .catch(log_numbered_error);
}

function populate_goals(r) {
    r.json()
    .then(j => {
        console.log("populate-goals response:", j);

        DATA.paces = new Map();
        DATA.goals = new Map();
        UTIL.clear(DISPLAY.calbox);

        j.sort((a, b) => {
            if(a.last < b.last) {
                return -1; 
            } else if(a.last > b.last) {
                return 1;
            } else if(a.rest < b.rest) {
                return -1;
            } else if(a.rest > b.rest) {
                return 1;
            } else {
                return 0;
            }
        })

        for(const p of j) {
            DATA.paces.set(p.uname, p);
            for(const g of p.goals) {
                g.uname = p.uname;
                DATA.goals.set(g.id, g);
            }

            const tab = make_calendar_table(p);
            DISPLAY.calbox.appendChild(tab);
        }
    })
    .catch(log_numbered_error);
}

function replace_pace(r) {
    r.json()
    .then(j => {
        console.log("update-pace response:", j);

        DATA.paces.set(j.uname, j);
        for(const g of j.goals) {
            g.uname = j.uname;
            DATA.goals.set(g.id, g);
        }

        const tab = make_calendar_table(j);
        const current_tab = document.querySelector(`table.pace[data-uname="${j.uname}"]`);
        current_tab.replaceWith(tab);
        if(current_tab.querySelector("tr.extra").style.display == "table-row") {
            tab.querySelector("button.expander").click();
        }
    })
    .catch(log_numbered_error);
}

function populate_dates(r) {
    r.json()
    .then(j => {
        console.log("populate-dates response:", j);

        DATA.dates = new Map();
        for(const [name, dstr] of Object.entries(j)) {
            DATA.dates.set(name, UTIL.iso2date(dstr));
        }
    })
    .catch(log_numbered_error);
}

function populate_traits(r) {
    r.json()
    .then(j => {
        console.log("populate-traits response:", j);

        DATA.traits = j;
        const cont = DISPLAY.sidecar_edit.querySelector("fieldset#trait-container");
        let n = 0;
        for(const trait of DATA.traits) {
            const fid = `edit-sidecar-trait-${n}`;
            const fipt = document.createElement("input");
            fipt.setAttribute("data-trait", trait);
            fipt.setAttribute("data-term", "fall");
            fipt.id = fid;
            const flab = document.createElement("label");
            flab.setAttribute("for", fid);
            flab.setAttribute("class", "r");
            UTIL.set_text(flab, trait);
            cont.appendChild(flab);
            cont.appendChild(fipt);

            n = n + 1;

            const sid = `edit-sidecar-trait-${n}`;
            const sipt = document.createElement("input");
            sipt.setAttribute("data-trait", trait);
            sipt.setAttribute("data-term", "spring");
            sipt.id = sid;
            const slab = document.createElement("label");
            slab.setAttribute("for", sid);
            slab.setAttribute("class", "l");
            UTIL.set_text(slab, trait);
            cont.appendChild(sipt);
            cont.appendChild(slab);

            n = n + 1;
        }

    })
    .catch(log_numbered_error);
}

async function show_sidecar(r) {
    let car = null;
    await r.json().then(j => { car = j; })
    .catch(e => {
        log_numbered_error(e);
        return;
    });

    const uname = car.uname;
    const pace = DATA.paces.get(car.uname);
    if(!pace) {
        const err_txt = `No data for student "${uname}"`;
        RQ.add_err(err_txt);
        return;
    }
    const form = document.forms["edit-sidecar"];

    form.elements["uname"].value = uname;

    const name = `${pace.rest} ${pace.last}`;
    UTIL.set_text(document.getElementById("edit-sidecar-meta"), name);

    for(const [oper, val] of Object.entries(car.facts)) {
        form.elements[oper].value = val;
    }

    const trait_inputs = form.querySelectorAll("input[data-trait]");
    for(const ipt of trait_inputs) {
        ipt.value = "";
    }

    for(const [trait, score] of Object.entries(car.fall_social)) {
        const ipt = form.querySelector(`input[data-term="fall"][data-trait="${trait}"]`);
        ipt.value = score;
    }
    for(const [trait, score] of Object.entries(car.spring_social)) {
        const ipt = form.querySelector(`input[data-term="spring"][data-trait="${trait}"]`);
        ipt.value = score;
    }

    form["complete-fall"].value = car.fall_complete;
    form["complete-spring"].value = car.spring_complete;

    const mastery_map = new Map();
    for(const m of car.mastery) {
        mastery_map.set(m.id, m.status);
    }

    const goal_rows = DISPLAY.sidecar_edit.querySelector("table#goal-mastery > tbody");
    UTIL.clear(goal_rows);
    for(const g of pace.goals) {
        const tr = document.createElement("tr");
        const course = DATA.courses.get(g.sym);
        const chapt = DATA.chapters.get(course.chapters[g.seq]);
        const ch_name = `${course.title} ${chapt.title}`;

        let td = document.createElement("td");
        UTIL.set_text(td, ch_name);
        tr.appendChild(td);

        td = document.createElement("td");
        UTIL.set_text(td, g.due || "");
        tr.appendChild(td);

        td = document.createElement("td");
        UTIL.set_text(td, g.done || "");
        tr.appendChild(td);

        td = document.createElement("td");
        UTIL.set_text(td, score2pct(g.score || "") || "");
        tr.appendChild(td);

        const ipt = document.createElement("select");
        ipt.setAttribute("data-id", g.id);
        for(const o of GOAL_MASTERY_OPTS) {
            let opt = document.createElement("option");
            opt.setAttribute("value", o.val);
            UTIL.set_text(opt, o.text);
            ipt.appendChild(opt);
        }
        const status = mastery_map.get(g.id);
        if(status) {
            ipt.value = status;
        } else if(g.done) {
            ipt.value = "Mastered";
        }
        td = document.createElement("td");
        td.appendChild(ipt);
        tr.appendChild(td);

        goal_rows.appendChild(tr);
    }

    DISPLAY.sidecar_edit.showModal();
}

document.getElementById("edit-sidecar-cancel")
    .addEventListener("click", evt => {
        evt.preventDefault();
        DISPLAY.sidecar_edit.close();
    });
document.getElementById("edit-sidecar-fall")
    .addEventListener("click", save_sidecar);
document.getElementById("edit-sidecar-spring")
    .addEventListener("click", save_sidecar);

function field_response(r) {
    if(!r.ok) {
        r.text()
        .then(t => {
            const err_txt = `${t}\n(${r.status}: ${r.statusText})`;
            RQ.add_err(err_txt);
        }
        ).catch(e => {
            const e_n = next_err();
            const err_txt = `Error #${e_n} (see console)`;
            console.log(`Error #${e_n}:`, e, r);
            RQ.add_err(err_txt);
        });

        return;
    }

    let action = r.headers.get("x-camp-action");

    if(!action) {
        const e_n = next_err();
        const err_txt = `Response lacked x-camp-action header. (See console error #${e_n}.)`;
        console.log(`Error #${e_n} response:`, r);
        RQ.add_err(err_txt);
        return;
    }
    switch(action) {
        case "populate-courses":
            populate_courses(r); break;
        case "populate-goals":
            populate_goals(r); break;
        case "update-pace":
            replace_pace(r); break;
        case "populate-dates":
            populate_dates(r); break;
        case "populate-traits":
            populate_traits(r); break;
        case "show-sidecar":
            show_sidecar(r); break;
        case "edit-markdown":
            edit_markdown(r); break;
        case "display-pdf":
            show_pdf(r); break;
        case "none":
            /* Don't do anything. This is a success that requires no action. */
            break;
        default:
            const e_n = next_err();
            const err_txt = `Unrecognized x-camp-action header: "${action}". (See console error #${e_n}.)`;
            console.log(`Error #${e_n} response:`, r);
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
        headers: headers
    };
    if(body) {
        const btype = typeof(body);
        if(btype == "string") {
            options.headers["content-type"] = "text/plain";
            options.body = body;
        } else if(btype == "object") {
            options.headers["content-type"] = "application/json";
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



UTIL.ensure_on_load(() => {
    request_action("populate-courses", "", "Fetching Course data.");
    request_action("populate-dates", "", "Fetching calendar events.");
    request_action("populate-traits", "", "Fetching list of social/emotional traits.");
});

document.getElementById("course-info-show")
    .addEventListener("click", () => {
        DISPLAY.course_list_div.style.display = "inline-flex";
        DISPLAY.course_list_hide.style.display = "inline-flex";
    });
DISPLAY.course_list_hide
    .addEventListener("click", () => {
        DISPLAY.course_list_div.style.display = "none";
        DISPLAY.course_list_hide.style.display = "none";
    });

/*


Now we get to the editing.


*/

document.getElementById("upload-goals")
    .addEventListener("click", () => {
        DISPLAY.upload_goals.showModal();
    })

function upload_goals_submit(evt) {
    evt.preventDefault();
    const form = document.forms["upload-goals"];
    const data = new FormData(form);
    const file = data.get("file");

    UTIL.get_file_as_text(file)
    .then(text => {
        DISPLAY.upload_goals.close();
        request_action("upload-goals", text, "Uploading new goals.");
    })
    .catch(err => {
        if(typeof(err) == "object") {
            console.log(err);
        }
        RQ.add_err(`Error opening local file: ${err}`);
    });
}

document.getElementById("upload-goals-confirm")
    .addEventListener("click", upload_goals_submit);
document.getElementById("upload-goals-cancel")
    .addEventListener("click", evt => {
        evt.preventDefault();
        DISPLAY.upload_goals.close();
    });

function populate_seq_list(evt) {
    const list = document.getElementById("course-seqs");
    const sym = document.forms["edit-goal"].elements["course"].value;
    const crs = DATA.courses.get(sym);

    UTIL.clear(list);
    const seqs = crs.chapters.filter(x => Boolean(x))
        .map(id => DATA.chapters.get(id).seq);
    for(const n in seqs) {
        const opt = document.createElement("option");
        opt.value = n;
        list.appendChild(opt);
    }
    console.log(seqs);

    const min = Math.min.apply(null, seqs);
    const max = Math.max.apply(null, seqs);

    DISPLAY.seq_input.setAttribute("min", min);
    DISPLAY.seq_input.setAttribute("max", max);

    let book_text = "";
    if (crs.book) { book_text = ` (${crs.book})`; }
    const course_text = `${crs.sym}: ${crs.title}${book_text}`;
    DISPLAY.course_input.setAttribute("title", course_text);
}

document.getElementById("edit-goal-course")
    .addEventListener("change", populate_seq_list);

function get_previous_goal(uname, goal_id) {
    const goals = DATA.paces.get(uname).goals;
    let prev_g = null;
    goals.some((g, n) => {
        if(g.id == goal_id) {
            prev_g = goals[n-1];
            return true;
        }
    });
    return prev_g;
}

function edit_goal(evt) {
    const form = document.forms["edit-goal"];
    const del = document.getElementById("delete-goal");
    let id = this.getAttribute("data-id");
    const confirm = document.getElementById("edit-goal-confirm");

    if(id) {
        id = Number(id);
        const g = DATA.goals.get(id);
        form.elements["id"].value = id;
        form.elements["course"].value = g.sym;
        form.elements["seq"].value = g.seq;
        form.elements["due"].value = g.due;
        form.elements["review"].checked = g.rev;
        form.elements["incomplete"].checked = g.inc;
        del.disabled = false;
        del.setAttribute("data-id", id);
        populate_seq_list();
        confirm.removeAttribute("data-uname");
    } else {
        for(const ipt of form.elements) {
            if(ipt.value) { ipt.value = null; }
            if(ipt.checked) { ipt.checked = false; }
        }
        del.disabled = true;
        del.removeAttribute("data-id");
        const uname = this.getAttribute("data-uname");
        confirm.setAttribute("data-uname", uname);

        const last_g = DATA.paces.get(uname).goals.at(-1);
        console.log(last_g);
        if(last_g) {
            const sym = last_g.sym;
            const next_seq = last_g.seq + 1;
            console.log(sym, next_seq);
            if(DATA.courses.get(sym)?.chapters[next_seq]) {
                form.elements["course"].value = sym;
                form.elements["seq"].value = next_seq;
            }
        }


    }

    DISPLAY.goal_edit.showModal();
}

function edit_goal_submit(evt) {
    const form = document.forms["edit-goal"];
    const uname = this.getAttribute("data-uname") || "";
    const id = Number(form.elements["id"].value) || 0;
    const sym = form.elements["course"].value?.trim() || "";
    const course = DATA.courses.get(sym);
    const seq = Number(form.elements["seq"].value) || 0;
    const chapt = course.chapters[seq];
    if(sym == "") {
        RQ,add_err("You must select a valid course.");
        return;
    } else if(!course) {
        RQ.add_err(`"${sym} is not a valid course symbol.`);
        return;
    }
    if(!chapt) {
        const err = `You must select a valid chapter number for course "${sym}": ${course.title} (${course.book}).`
        RQ.add_err(err);
        return;
    }

    // Pre-fill default values for a new goal.
    let g = {
        "uname": uname,
        "done": null,
        "tries": null,
        "weight": 0,
        "score": null,
    };

    // If this is an extant goal, pre-fill all the extant data.
    if(form.elements["id"].value) {
        for(const [k, v] of Object.entries(DATA.goals.get(id))) {
            g[k] = v;
        }
    }

    g["id"] = id;
    g["sym"] = sym;
    g["seq"] = seq;
    g["rev"] = form.elements["review"].checked;
    g["inc"] = form.elements["incomplete"].checked;
    g["due"] = form.elements["due"].value || null;

    DISPLAY.goal_edit.close();
    if(form.elements["id"].value) {
        request_action("update-goal", g, `Updating Goal ${id}`);
    } else {
        request_action("add-goal", g, `Adding new Goal: ${sym}, ${seq} for ${uname}`);
    }
    
}

document.getElementById("edit-goal-cancel")
    .addEventListener("click", (evt) => {
        evt.preventDefault(),
        DISPLAY.goal_edit.close();
    });
document.getElementById("edit-goal-confirm")
    .addEventListener("click", edit_goal_submit);

async function delete_goal_submit(evt) {
    const id = this.getAttribute("data-id");
    const g = DATA.goals.get(Number(id));
    const crs = DATA.courses.get(g.sym);
    const chp = DATA.chapters.get(crs.chapters[g.seq]);
    const q = `Are you sure you want to delete ${crs.title} ${chp.title} for ${g.uname}?.`;
    if(await are_you_sure(q)) {
        DISPLAY.goal_edit.close();
        request_action("delete-goal", id, `Deleting Goal #${id}.`);
    }
}

document.getElementById("delete-goal")
    .addEventListener("click", delete_goal_submit);

function complete_goal(evt) {
    const id = this.getAttribute("data-id");
    const form = document.forms["complete-goal"];
    const g = DATA.goals.get(Number(id));

    form.elements["id"].value = id;
    if(g.done) {
        form.elements["done"].value = g.done;
    } else {
        form.elements["done"].value = UTIL.date2iso(new Date());
    }
    form.elements["tries"].value = g.tries;
    form.elements["score"].value = g.score;

    DISPLAY.goal_complete.showModal();
}

function complete_goal_submit(evt) {
    const form = document.forms["complete-goal"];
    const data = new FormData(form);
    const id = Number(data.get("id"));
    const g = DATA.goals.get(id);

    const valid_score = Boolean(interpret_score(data.get("score")));
    const valid_date = (UTIL.iso2date(data.get("done")) != "Invalid Date");
    let score = null;
    let done = null;
    if(UTIL.iso2date(data.get("done")) != "Invalid Date") {
        done = data.get("done");
    }
    let tries = Number(data.get("tries"));
    if(!tries) { tries = null; }

    if(!(valid_score == valid_date)) {
        console.log(score, done);
        RQ.add_err("A valid completion date requires a valid score (and vice-versa).");
        return;
    }

    if(valid_score) {
        if(!tries) { tries = 1;}
        score = data.get("score");
    } else {
        tries = null;
    }

    g.done = done;
    g.score = score;
    g.tries = tries;

    DISPLAY.goal_complete.close();
    request_action("update-goal", g, `Marking Goal $${g.id} complete.`);
}

document.getElementById("complete-goal-cancel")
    .addEventListener("click", (evt => {
        evt.preventDefault();
        DISPLAY.goal_complete.close();
    }));
document.getElementById("complete-goal-confirm")
    .addEventListener("click", complete_goal_submit);


function update_numbers_submit(evt) {
    evt.preventDefault();
    const uname = this.getAttribute("data-uname");
    const data = new FormData(document.forms[`${uname}-extra`]);
    const cal = JSON.parse(JSON.stringify(DATA.paces.get(uname)));
    cal.goals = [];

    cal.fnot = Number(data.get("fall-notices"));
    cal.snot = Number(data.get("spring-notices"));
    cal.fex = null;
    cal.sex = null;
    cal.fex_frac = Number(data.get("fall-exam-frac"));
    cal.sex_frac = Number(data.get("spring-exam-frac"));

    const fex = data.get("fall-exam").trim();
    if(fex) {
        if(interpret_score(fex)) {
            cal.fex = fex;
        } else {
            RQ.add_err(`"${fex}" is not a valid Fall Exam score.`);
        }
    }

    const sex = data.get("spring-exam").trim();
    if(sex) {
        if(interpret_score(sex)) {
            cal.sex = sex;
        } else {
            RQ.add_err(`${sex} is not a valid Spring Exam score.`);
        }
    }

    request_action("update-numbers", cal, `Updating scores for ${cal.first} ${cal.rest}.`);
}

async function autopace(evt) {
    evt.preventDefault();
    const uname = this.getAttribute("data-uname");
    const cal = DATA.paces.get(uname);
    const q = `This operation will probably change all due dates for ${cal.rest} ${cal.last}.`;
    if(await are_you_sure(q)) {
        request_action("autopace", uname, `Autopacing due dates for ${cal.rest} ${cal.last}.`);
    }
}

async function clear_goals(evt) {
    evt.preventDefault();
    const uname = this.getAttribute("data-uname");
    const cal = DATA.paces.get(uname);
    const q = `This operation will totally and unrecoverably nuke the calendar for ${cal.rest} ${cal.last}.`;
    if(await are_you_sure(q)) {
        request_action("clear-goals", uname, `Clearing goals for ${cal.rest} ${cal.last}.`);
    }
}

function edit_sidecar(evt) {
    evt.preventDefault();
    const uname = this.getAttribute("data-uname");
    const cal = DATA.paces.get(uname);
    request_action("show-sidecar", uname, `Fetching report data sidecar for ${cal.rest} ${cal.last}`);
}

function save_sidecar(evt) {
    evt.preventDefault();
    const term = this.getAttribute("data-term");
    const form = document.forms["edit-sidecar"];
    const data = new FormData(form);
    const uname = data.get("uname");

    const sc = { "uname": uname };

    const fact_fieldset = form.querySelector("fieldset#fact-mastery-container");
    const facts_inputs = fact_fieldset.querySelectorAll("select");
    const facts = {};
    for(const ipt of facts_inputs) {
        facts[ipt.name] = ipt.value;
    }
    sc["facts"] = facts;

    let trait_fieldset = form.querySelector("fieldset#trait-container");
    for(const term of ["fall", "spring"]) {
        const trait_inputs = trait_fieldset.querySelectorAll(`input[data-term="${term}"]`);
        const social = {};
        for(const ipt of trait_inputs) {
            social[ipt.getAttribute("data-trait")] = ipt.value;
        }
        const name = `${term}_social`;
        sc[name] = social;
    }

    sc["fall_complete"] = form["complete-fall"].value.trim();
    sc["spring_complete"] = form["complete-spring"].value.trim();
    sc["summer_complete"] = form["complete-summer"].value.trim();

    let mastery_fieldset = form.querySelector("fieldset#goal-mastery-container");
    let mastery_inputs = mastery_fieldset.querySelectorAll("select");
    let mastery = Array.from(mastery_inputs).map(ipt => {
        const goal_id = Number(ipt.getAttribute("data-id"))
        const g = DATA.goals.get(goal_id);
        if(g.done) {
            return { "id": goal_id, "status": ipt.value };
        } else {
            return null;
        }
    }).filter(x => Boolean(x));
    sc["mastery"] = mastery;

    const extra_headers = {
        "x-camp-term": term,
    };

    const p = DATA.paces.get(uname);
    const msg = `Saving report data sidecar for ${p.rest} ${p.last}.`;
    DISPLAY.sidecar_edit.close();
    request_action("update-sidecar", sc, msg, extra_headers);
}

function edit_markdown(r) {
    r.text()
    .then(text => {
        const form = document.forms["edit-report"];
        const textarea = form.elements["text"];
        textarea.value = text;
        const uname = r.headers.get("x-camp-student");
        const term = r.headers.get("x-camp-term");
        const butt = form.elements["save"];
        if(uname) {
            butt.setAttribute("data-uname", uname);
        } else {
            RQ.set_err("Report markdown response has no x-camp-student header value.")
            return;
        }
        butt.setAttribute("data-term", term);
        DISPLAY.report_edit.showModal();
    })
    .catch(log_numbered_error);
}

async function save_markdown(evt) {
    const form = document.forms["edit-report"];
    const data = new FormData(form);
    const text = data.get("text");
    let term = form.elements["save"].getAttribute("data-term");
    let uname = form.elements["save"].getAttribute("data-uname");
    DISPLAY.report_edit.close();

    const extra_headers = {
        "x-camp-student": uname,
        "x-camp-term": term,
    };
    const pace = DATA.paces.get(uname);
    const desc = `Generating ${term} report for ${pace.rest} ${pace.last}.`;
    request_action("render-report", text, desc, extra_headers);
}

document.getElementById("edit-report-cancel")
    .addEventListener("click", (evt => {
        evt.preventDefault();
        DISPLAY.report_edit.close();
    }));
document.getElementById("edit-report-save")
    .addEventListener("click", save_markdown);

function show_pdf(r) {
    r.blob()
    .then(blob => {
        const form = document.forms["view-pdf"];
        const uname = r.headers.get("x-camp-student");
        const term = r.headers.get("x-camp-term");
        const butt = form.elements["discard"];
        butt.setAttribute("data-uname", uname);
        butt.setAttribute("data-term", term);
        const obj = document.getElementById("view-pdf-object");
        const url = window.URL.createObjectURL(blob);
        obj.data = url;
        DISPLAY.pdf_view.showModal();
    })
    .catch(log_numbered_error);
}

function discard_pdf(evt) {
    evt.preventDefault();
    const uname = this.getAttribute("data-uname");
    const term = this.getAttribute("data-term");
    DISPLAY.pdf_view.close();

    const extra_headers = {
        "x-camp-student": uname,
        "x-camp-term": term,
    };
    const pace = DATA.paces.get(uname);
    const desc = `Discarding ${term} report for ${pace.rest} ${pace.last}.`;
    request_action("discard-pdf", null, desc, extra_headers);
}

document.getElementById("view-pdf-cancel")
    .addEventListener("click", discard_pdf);
document.getElementById("view-pdf-save")
    .addEventListener("click", (evt => {
        evt.preventDefault();
        DISPLAY.pdf_view.close();
    }));