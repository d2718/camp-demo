/*
Oh, man, we're implementing a calendar.
*/
"use strict";

const CAL = {
    dates: new Set(),
    include_on_drag: true,
    has_populated: false,
    target_div: document.getElementById("calendar-display"),
    year_selector: document.getElementById("cal-year"),
    date_form: document.forms["cal-dates-form"],
    month_names: {
        0: "Jan",
        1: "Feb",
        2: "Mar",
        3: "Apr",
        4: "May",
        5: "Jun",
        6: "Jul",
        7: "Aug",
        8: "Sep",
        9: "Oct",
        10: "Nov",
        11: "Dec",
    },
    current_academic_year: function() {
        const today = new Date();
        if(today.getMonth() < 7) {
            return today.getFullYear() - 1;
        } else {
            return today.getFullYear();
        }
    },
};

CAL.toggle_on_mousedown = function() {
    const date = this.getAttribute("data-date");
    if(CAL.dates.delete(date)) {
        this.removeAttribute("class");
        CAL.include_on_drag = false;
    } else {
        this.setAttribute("class", "y");
        CAL.dates.add(date);
        CAL.include_on_drag = true;
    }
}
CAL.set_on_drag = function(evt) {
    if(evt.buttons == 1) {
        const date = this.getAttribute("data-date");
        if(CAL.include_on_drag) {
            CAL.dates.add(date);
            this.setAttribute("class", "y");
        } else {
            CAL.dates.delete(date);
            this.removeAttribute("class");
        }
    }
}

CAL.make_month = function(year, month) {
    const tab = document.createElement("table");
    tab.setAttribute("class", "calendar-month");

    const thead = document.createElement("thead");

    const title_row = document.createElement("tr");
    const title_th = document.createElement("th");
    title_th.setAttribute("colspan", "7");
    title_th.appendChild(document.createTextNode(
        `${CAL.month_names[month]} ${year}`
    ));
    title_row.appendChild(title_th);
    thead.appendChild(title_row);

    const thr = document.createElement("tr");
    for(const d of ['S', 'M', 'T', 'W', 'R', 'F', 'S']) {
        const th = document.createElement("th");
        th.appendChild(document.createTextNode(d));
        thr.appendChild(th);
    }
    thead.appendChild(thr);
    tab.appendChild(thead);

    const tbody = document.createElement("tbody");

    const first_day = new Date(year, month, 1);
    let current_tr = null;
    if(first_day.getDay() > 0) {
        current_tr =  document.createElement("tr");
        for(let n = 0; n < first_day.getDay(); n++) {
            current_tr.appendChild(document.createElement("td"));
        }
    }
    let day_n = 1;
    let current_day = new Date(year, month, day_n);
    while(current_day.getMonth() == month) {
        if(current_day.getDay() == 0) {
            current_tr = document.createElement("tr");
        }
        const td = document.createElement("td");
        td.setAttribute("data-date", UTIL.date2iso(current_day));
        td.addEventListener("mousedown", CAL.toggle_on_mousedown);
        td.addEventListener("mouseover", CAL.set_on_drag);
        td.appendChild(document.createTextNode(current_day.getDate()));
        current_tr.appendChild(td);
        if(current_day.getDay() == 6) {
            tbody.appendChild(current_tr);
            current_tr = null;
        }
        day_n = day_n + 1;
        current_day = new Date(year, month, day_n);
    }
    if(current_tr) {
        tbody.appendChild(current_tr);
        current_tr = null;
    }

    tab.appendChild(tbody);
    return tab;
}

CAL.populate_year = function(target_elt, year) {
    UTIL.clear(target_elt);

    for(let m = 7; m < 12; m++) {
        target_elt.appendChild(
            CAL.make_month(year, m)
        );
    }
    for(let m = 0; m < 7; m++) {
        target_elt.appendChild(
            CAL.make_month(year+1, m)
        );
    }
}

CAL.repaint_dates = function() {
    for(const td of CAL.target_div.querySelectorAll("td[data-date]")) {
        if(CAL.dates.has(td.getAttribute("data-date"))) {
            td.setAttribute("class","y");
        } else {
            td.removeAttribute("class");
        }
    }

    for(const td of CAL.target_div.querySelectorAll("td[data-special]")) {
        td.removeAttribute("data-special");
    }
    for(const input of CAL.date_form.elements) {
        const td = CAL.target_div.querySelector(`td[data-date="${input.value}"]`);
        if(td) {
            td.setAttribute("data-special", input.name);
        }
    }
}

CAL.set_local = function(r) {
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
        });
        return;
    }

    let action = r.headers.get("x-camp-action");

    if(!action) {
        RQ.add_err("Response lacked x-camp-action header.");
    } else if(action == "populate-cal") {
        r.json()
        .then(j => {
            CAL.dates = new Set(j);
            CAL.repaint_dates();
        }).catch(e => {
            console.log(e.line, e);
            RQ.add_err(e);
        });
    } else {
        RQ.add_err(`Unrecognized x-camp-action header value: ${action}.`);
    }
}

CAL.populate_dates = function(r) {
    r.json()
    .then(j => {
        console.log("populate-dates body:", j)
        for(const input of CAL.date_form.elements) {
            input.value = "";
        }
        for(const [date_name, date_str] of Object.entries(j)) {
            CAL.date_form.elements[date_name].value = date_str;
        }
        CAL.repaint_dates();
    })
    .catch(RQ.add_err)
}

CAL.update_date = function(evt) {
    const data = [this.name, this.value];
    CAL.request_action("set-date", data, `Setting ${this.name}.`);
}

CAL.field_response = function(r) {
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
        const err_txt = `CAL: Response lacked x-camp-action header. (See console error #${e_n}.)`;
        console.log(e_n, r);
        RQ.add_err(err_txt);

    } else if(action == "populate-cal") {
        CAL.set_local(r);
    } else if(action == "populate-dates") {
        CAL.populate_dates(r);
    } else {
        const e_n = STATE.next_error();
        const err_txt = `CAL: Unrecognized x-camp-action header: ${action}. (See console error #${e_n})`;
        console.log(e_n, r);
        RQ.add_err(err_txt);
    }
}

CAL.request_action = function(action, body, description) {
    const options = {
        method: "POST",
        headers: {
            "x-camp-action": action,
        }
    };

    if(body) {
        const bt = typeof(body);
        if(bt == "string") {
            options.headers["content-type"] = "text/plain";
            options.body = body;
        } else if(bt == "object") {
            options.headers["content-type"] = "application/json";
            options.body = JSON.stringify(body);
        }
    }

    const r = new Request(
        API_ENDPOINT,
        options
    );

    const desc = (description || action);
    api_request(r, desc, CAL.field_response);
}

CAL.update_cal = function() {
    for (const td of CAL.target_div.querySelectorAll('td[class="y"]')) {
        td.setAttribute("class", "m");
    }

    CAL.request_action("update-cal", Array.from(CAL.dates), "Updating calendar.")
}

document.getElementById("cal-prev-year")
    .addEventListener("click", () => {
        const new_year = Number(CAL.year_selector.value) - 1;
        CAL.year_selector.value = new_year;
        CAL.populate_year(CAL.target_div, new_year);
        CAL.repaint_dates();
    })
document.getElementById("cal-next-year")
    .addEventListener("click", () => {
        const new_year = Number(CAL.year_selector.value) + 1;
        CAL.year_selector.value = new_year;
        CAL.populate_year(CAL.target_div, new_year);
        CAL.repaint_dates();
    })
CAL.year_selector.addEventListener("change", function(evt) {
    CAL.populate_year(CAL.target_div, Number(this.value));
    CAL.repaint_dates();
})
for(const input of CAL.date_form.elements) {
    input.addEventListener("change", CAL.update_date);
}

document.getElementById("cal-tab-radio")
    .addEventListener("change", () => {
        if(!CAL.has_populated) {
            CAL.has_populated = true;
            const cur_year = CAL.current_academic_year();
            CAL.year_selector.value = cur_year;
            CAL.populate_year(CAL.target_div, cur_year);
            CAL.request_action("populate-cal", "", "Fetching calendar.");
            CAL.request_action("populate-dates", "", "Fetching dates.")
        }
});
document.getElementById("cal-update")
    .addEventListener("click", CAL.update_cal);