# spotty
*spotty* is an open source client application for Spotify. It's based on and using 
[librespot](https://github.com/librespot-org/librespot). It basically is a stripped 
down and slightly customized version of the librespot sample application.

*spotty* has been tweaked to enable interaction with the [Logitech Media Server](https://github.com/Logitech/slimserver).

* allow piping of a single track's audio data to LMS' transcoding framework (`--single-track`)
* optionally start stream from given position in seconds (`--start-position 123`)
* tell spotty in daemon mode how to notify LMS about state changes (`--lms {ip address}` and `--player-mac {MAC address}`)
* get a token to be used with the [Spotify Web API](https://developer.spotify.com/web-api/) (`--get-token`) using a given client-id and scope (`--client-id abcd-...`, `--scope ...`)

In order to enable all these features it uses a slightly [customized librespot](https://github.com/michaelherger/librespot/tree/spotty) to be found on my GitHub account.

## Disclaimer
Using this code to connect to Spotify's API is probably forbidden by them.
Use at your own risk.

## License
Everything in this repository is licensed under the MIT license.

