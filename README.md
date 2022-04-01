# Twitter Downloader

[![Build status](https://github.com/jacob-pro/twitter-dl/actions/workflows/rust.yml/badge.svg)](https://github.com/jacob-pro/twitter-dl/actions)
![maintenance-status](https://img.shields.io/badge/maintenance-experimental-blue.svg)

## Twitter API Info

This requires access to the Twitter API (which means creating a developer account).

At present there are two main API versions, neither of which are good:

| API Version | Supports Photos | Supports Videos | Supports Gifs | Requires Account Approval |
|-------------|-----------------|-----------------|---------------|---------------------------|
| 1.1         | ✅               | ✅               | ✅             | Yes                       |
| 2.0         | ✅               | ❌               | ❌             | No                        |

## Limitations

- Doesn't support private accounts.
- Can only download up to 3200 tweets (API limitations).
- No option to download retweets.

## Install

From latest git:
```
cargo install --git https://github.com/jacob-pro/twitter-dl
```

## Usage

First create an `auth.json` file containing your `{ "bearer_token": "$TOKEN" }`

Download Twitter account(s):

```shell
twitter-dl download --out  ./twitter --users $USERNAMES --photos --videos --gifs 
```

View the downloaded tweets in a basic web app:

```shell
twitter-dl serve --dir  ./twitter
```

For full usage try the `--help` command.
