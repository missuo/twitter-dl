$( document ).ready(function() {
    const BLOG_CHOICE = $("#blog-choice");
    const SEARCH = $("#search");
    const PAGE_CHOICE = $("#page-choice");
    const FORM = $("#form")
    const POSTS_DIV = $("#posts");
    const PAGE_SIZE = 100;
    const TYPE = $("#type")
    const TOTAL = $("#total")
    const SHOWING = $("#showing")
    const SORT = $("#sort")

    let ALL_TWEETS = [];
    let FILTERED_TWEETS = [];

    FORM.trigger("reset");
    FORM.submit(function( event ) {
        event.preventDefault();
    });

    $.get("/list").then(
        function(list) {
            if (list.length < 1) {
                throw new Error("No downloaded twitters found")
            }
            BLOG_CHOICE.empty();
            const placeholder = new Option("Choose an account", "");
            placeholder.setAttribute('disabled', true);
            placeholder.setAttribute('selected', true);
            BLOG_CHOICE.append(placeholder);
            list.forEach((d) => BLOG_CHOICE.append(new Option(d,d)));
            BLOG_CHOICE.attr('disabled' , false);
        },
        function (e) {
            throw new Error("Get /list failed")
        }
    ).catch((e) => {
        alert(e);
    })

    BLOG_CHOICE.change(function() {
        const blog = $(this).val();
        const base = `/dir/${blog}`;
        const url = `${base}/tweets.json`
        $.get(url).then(
            (res) => {
                ALL_TWEETS = res.tweets.map((tweet) => Tweet.deserialize(tweet, base))
                TOTAL.text(`Total: ${ALL_TWEETS.length}`)
                apply_filters();
                update_page_choice();
                render_posts();
            },
            function (e) {
                throw new Error(`Get ${url} failed`)
            }
        ).catch((e) => {
            alert(e);
        })
    });

    PAGE_CHOICE.change(function() {
        apply_filters();
        render_posts();
    });

    TYPE.change(function() { refresh() });

    SORT.change(function() { refresh() });

    SEARCH.on("input", function(e) {
        clearTimeout(this.thread);
        this.thread = setTimeout(function() {
            refresh()
        }, 150);
    });

    function refresh() {
        apply_filters();
        update_page_choice();
        render_posts();
    }

    // Filters by search, type and sorting
    function apply_filters() {
        const search = SEARCH[0].value;
        const type = TYPE[0].value;
        const sort = SORT[0].value;
        FILTERED_TWEETS = ALL_TWEETS.filter((p) => {
            return type === "All" || p.matches_type(type)
        })
        FILTERED_TWEETS = FILTERED_TWEETS.filter((p) => {
            return search.length === 0 || p.matches_search(search)
        })
        FILTERED_TWEETS.sort(function (a, b) {
            if (sort === "Oldest") {
                return a.id - b.id;
            } else {
                return b.id - a.id;
            }
        });
    }

    function update_page_choice() {
        PAGE_CHOICE.empty()
        if (FILTERED_TWEETS.length < 1) {
            const placeholder = new Option("1", "1");
            placeholder.setAttribute('disabled', true);
            PAGE_CHOICE.append(placeholder)
            PAGE_CHOICE.attr('disabled' , true);
        } else {
            const pages = Math.ceil(FILTERED_TWEETS.length / PAGE_SIZE)
            for (let i = 1; i <= pages; i++) {
                PAGE_CHOICE.append(new Option(i.toString(), i.toString()));
            }
            PAGE_CHOICE.attr('disabled' , false);
        }
    }

    function render_posts() {
        POSTS_DIV.empty();
        const page_number = parseInt(PAGE_CHOICE[0].value) - 1;
        const start = page_number * PAGE_SIZE
        const stop = (page_number + 1) * PAGE_SIZE
        const tweets = FILTERED_TWEETS.slice(start, stop);
        SHOWING.text(`Showing: ${tweets.length}`)
        for (const tweet of tweets) {
            const render = tweet.render();
            POSTS_DIV.append(`<div class='post' id="${tweet.id}">${render}</div>`)
        }
    }

});

class Tweet {
    id;
    date;
    text
    media;

    constructor(id, date, text, media) {
        this.id = id
        this.date = date
        this.text = text;
        this.media = media
    }

    static deserialize(object, base) {
        const date = new Date(object.timestamp * 1000).toLocaleString();
        const media = object.media.map((m) => Media.deserialize(m, base))
        return new Tweet(object.id, date, object.text, media)
    }

    matches_search(search) {
        return this.text.includes(search)
    }

    matches_type(type) {
        return this.media.some((m) => m.type === type)
    }

    render() {
        const medias = this.media.map((m) => m.render());
        return [
            `<p>${this.date}</p>`,
            `<p>${this.text}</p>`,
            ...medias,
        ].join("\n")
    }

}

class Media {
    type;
    url;

    constructor(type, url) {
        this.type = type;
        this.url = url;
    }

    static deserialize(object, base) {
        const filename = object.file_name;
        const url = filename === null ? null : `${base}/${filename}`;
        return new Media(object.type, url);
    }

    render() {
        if (!this.url) {
            return `<p>${this.type} not downloaded</p>`;
        } else if (this.type === "video" || this.type === "gif") {
            return `<video controls preload="metadata"><source src="${this.url}"></video>`;
        } else if (this.type === "photo") {
            return `<img src="${this.url}" alt="">`;
        }
    }

}
