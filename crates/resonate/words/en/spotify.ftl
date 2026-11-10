signed-in = signed in to Spotify; the refresh token is kept in { $settings }
follow-or-move = `resonate spotify --follow` keeps the account's playlists in step, `resonate spotify --move` copies them once
signed-out = signed out of Spotify; nothing more is asked of the account, and { $playlists ->
    [one] { $playlists } playlist
   *[other] { $playlists } playlists
} kept the rows and no longer followed it
open-to-sign-in = open this address to sign in to Spotify, with { $redirect } listed among the app's redirect URIs:
