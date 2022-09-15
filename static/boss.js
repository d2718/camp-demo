
"use strict";

const API_ENDPOINT = "/boss";

const SORTS = {
    "name": (a, b) => a.getAttribute("data-name").localeCompare(b.getAttribute("data-name")),
    "teacher": (a, b) => a.getAttribute("data-tname").localeCompare(b.getAttribute("data-tname")),
    "lag": (a, b) => Number(a.getAttribute("data-lag")) - Number(b.getAttribute("data-lag")),
};

function toggle_table_body(evt) {
    const tab = this.parentElement;
    const body = tab.querySelector("tbody");
    if(body.style.display == "table-row-group") {
        body.style.display = "none";
    } else {
        body.style.display = "table-row-group";
    }
}

function sort_tables(cmpfuncs) {
    const tab_arr = new Array();
    const cal_div = document.getElementById("cals");

    while(cal_div.firstChild) {
        tab_arr.push(cal_div.removeChild(cal_div.lastChild));
    }

    for(const f of cmpfuncs) {
        tab_arr.sort(f);
    }

    console.log(tab_arr);

    for(const tab of tab_arr) {
        cal_div.appendChild(tab);
    }
}

for(const tab of document.querySelectorAll("table")) {
    tab.querySelector("thead").addEventListener("click", toggle_table_body);
}

document.getElementById("name").addEventListener("click",
    () => sort_tables([SORTS.name])
);
document.getElementById("teacher").addEventListener("click",
    () => sort_tables([SORTS.name, SORTS.teacher])
);
document.getElementById("lag").addEventListener("click",
    () => sort_tables([SORTS.name, SORTS.lag])
);

sort_tables([SORTS.name]);

const DISPLAY = {
    "edit_dialog": document.getElementById("edit-email"),
    "email_text": document.getElementById("email-text"),
    "email_edit_submit": document.getElementById("edit-email-confirm"),
}

function edit_email(r) {
    r.json()
    .then(j => {
        
        const name_span = document.getElementById("email-subject");
        UTIL.set_text(name_span, j.student_name);
        DISPLAY.email_text.value = j.text;
        DISPLAY.email_edit_submit.setAttribute("data-uname", j.uname);
        DISPLAY.edit_dialog.showModal();

    }).catch(RQ.add_err)
}

function field_response(r) {
    if(!r.ok) {
        r.text()
        .then(t => {
            const err_text = `${t}\n(${r.status}: ${r.statusText})`;
            RQ.add_err(err_text);
        }).catch(e => {
            console.log("Uncaught error:", e);
            RQ.add_err("Error processing response (see console).");
        });

        return;
    }

    let action = r.headers.get("x-camp-action");

    if(!action) {
        console.log("Response lacks x-camp-action header:", r);
        RQ.add_err("Response lacked x-camp-action header. (See console).");

    } else if(action == "edit-email") {
        edit_email(r);
    } else if(action == "none") {
        // No action required, obviously.
    } else {
        const estr = `Unrecognized x-camp-action header: ${action}`;
        console.log(estr, r);
        RQ.add_err(extr + " (see console).");
    }
}


function request_action(action, body, description) {
    const options = {
        method: "POST",
        headers: { "x-camp-action": action }
    };
    if(body) {
        const body_type = typeof(body);
        if(body_type == "string") {
            options.headers["content-type"] = "text/plain";
            options.body = body;
        } else if (body_type == "object") {
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

function generate_email(evt) {
    evt.preventDefault();
    const uname = this.getAttribute("data-uname");
    request_action("compose-email", uname, "Generating parent email.");
}

for(const butt of document.querySelectorAll("tr.extra button[data-uname]")) {
    butt.addEventListener("click", generate_email);
}

document.getElementById("edit-email-cancel")
    .addEventListener("click", evt => {
        evt.preventDefault();
        DISPLAY.edit_dialog.close();
    })

function send_email(evt) {
    evt.preventDefault();

    const body = {
        "uname": this.getAttribute("data-uname"),
        "text": DISPLAY.email_text.value,
    };

    DISPLAY.edit_dialog.close();

    request_action("send-email", body, "Sending email.");
}

DISPLAY.email_edit_submit.addEventListener("click", send_email);
document.getElementById("edit-email-cancel")
    .addEventListener("click", evt => {
        evt.preventDefault();
        DISPLAY.email_edit_submit.close();
    });

document.getElementById("email-all").addEventListener("click", async () => {
    const q = "Are you absolutely sure you want to spam everyone's parents with their progress at this time?";
    if(await are_you_sure(q)) {
        request_action("email-all", null, "Emailing all parents.");
    }
})