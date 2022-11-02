
"use strict";

const API_ENDPOINT = "/boss";

// Regex for extracting filename from Content-Disposition header.
const FILENAME = /; filename="([^"]+)"/;
// Time (in ms) to wait for an object to start downloading before its
// ObjectURL is revoked.
const DOWNLOAD_DELAY = 5000;

// Sorting functions for sorting pace calendar tables.
const SORTS = {
    "name": (a, b) => a.getAttribute("data-name").localeCompare(b.getAttribute("data-name")),
    "teacher": (a, b) => a.getAttribute("data-tname").localeCompare(b.getAttribute("data-tname")),
    "lag": (a, b) => Number(a.getAttribute("data-lag")) - Number(b.getAttribute("data-lag")),
};

/* Expand/collapse a table's body.

Should be set as an event handler to fire when a table's head is clicked.
*/
function toggle_table_body(evt) {
    const tab = this.parentElement;
    const body = tab.querySelector("tbody");
    if(body.style.display == "table-row-group") {
        body.style.display = "none";
    } else {
        body.style.display = "table-row-group";
    }
}

// Add the expand/collapse event handler to every table's head.
for(const tab of document.querySelectorAll("table")) {
    tab.querySelector("thead").addEventListener("click", toggle_table_body);
}

/* Event handler for table sorting buttons. */
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

// Add sorting event handlers to the sort buttons.
document.getElementById("name").addEventListener("click",
    () => sort_tables([SORTS.name])
);
document.getElementById("teacher").addEventListener("click",
    () => sort_tables([SORTS.name, SORTS.teacher])
);
document.getElementById("lag").addEventListener("click",
    () => sort_tables([SORTS.name, SORTS.lag])
);

// Sort tables by name initially.
sort_tables([SORTS.name]);

// Sort archive-downloading buttons so they appear in a consistent order.
{
    const container = document.getElementById("archive-buttons");
    const butt_arr = new Array();
    while(container.firstChild) {
        const elt = container.removeChild(container.lastChild);
        if(elt.tagName) {
            butt_arr.push(elt);
        }
    }
    console.log(butt_arr);
    butt_arr.sort((a, b) => a.getAttribute("data-uname").localeCompare(b.getAttribute("data-uname")));
    for(const butt of butt_arr) {
        container.appendChild(butt);
    }
}

// UI elements to which we want easy access.
const DISPLAY = {
    "edit_dialog": document.getElementById("edit-email"),
    "email_text": document.getElementById("email-text"),
    "email_edit_submit": document.getElementById("edit-email-confirm"),
    "pdf_view": document.getElementById("view-pdf",)
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

function display_pdf(r) {
    r.blob()
    .then(blob => {
        const form = document.forms["view-pdf"];
        const obj = document.getElementById("view-pdf-object");
        const url = window.URL.createObjectURL(blob);
        obj.data = url;
        DISPLAY.pdf_view.showModal();
    })
    .catch(e => {
        console.log(e),
        RQ.add_err("There was an error displaying the PDF; see console for details.");
    });
}

function save_archive(r) {
    r.blob()
    .then(blob => {
        const fname = r.headers.get("Content-Disposition").match(FILENAME)[1];
        const file_url = window.URL.createObjectURL(blob);
        const link = document.createElement("A");
        link.href = file_url;
        link.download = fname;
        link.click();
        // Give it a chance to get the download started. Is this hacky? Yes.
        setTimeout(() => window.URL.revokeObjectURL(file_url), DOWNLOAD_DELAY);
    })
    .catch(e => {
        console.log(e),
        RQ.add_ERR("There was an error downloading the ZIP archive of reports; see the console for details.");
    });
}

function field_response(r) {
    if(!r.ok) {
        r.text()
        .then(t => {
            const status = r.statusText || "[no reason phrase (HTTP/2 is boring)]";
            const err_text = `${t}\n(${r.status}: ${status})`;
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
        return;
    }
    switch(action) {
        case "edit-email":
            edit_email(r); break;
        case "download-pdf":
            display_pdf(r); break;
        case "download-archive":
            save_archive(r); break;
        case "none": /* No action required, obviously. */
            break;
        default:
            const estr = `Unrecognized x-camp-action header: ${action}`;
            console.log(estr, r);
            RQ.add_err(estr + " (see console).");
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

for(const butt of document.querySelectorAll("tr.extra button.send-email")) {
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

function download_report(evt) {
    evt.preventDefault();
    const uname = this.getAttribute("data-uname");
    const term = this.getAttribute("data-term");
    const extra_headers = {
        "x-camp-student": uname,
        "x-camp-term": term,
    };

    const desc = `Downloading ${term} report for ${uname}.`;
    request_action("download-report", null, desc, extra_headers);
}

for(const butt of document.querySelectorAll("tr.extra button.download-report")) {
    butt.addEventListener("click", download_report);
}

document.getElementById("view-pdf-cancel")
    .addEventListener("click", evt => {
        evt.preventDefault();
        DISPLAY.pdf_view.close();
    })


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

function download_archive(evt) {
    evt.preventDefault();
    let uname = this.getAttribute("data-uname");
    let form = document.forms["archives"];
    let data = new FormData(form);
    let term = data.get("term");
    const extra_headers = {
        "x-camp-teacher": uname,
        "x-camp-term": term,
    };

    const desc = `Downloading all ${term} reports generated by ${uname}.`
    request_action("report-archive", null, desc, extra_headers);
}

for(const butt of document.querySelectorAll("div#archive-buttons button")) {
    butt.addEventListener("click", download_archive);
}