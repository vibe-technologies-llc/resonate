queued = { $title } est en attente
already-held = { $title } est déjà tenu, donc rien n’a été attendu
list-kept = { $name }: { $queued ->
    [one] { $queued } chanson
   *[other] { $queued } chansons
} en attente, { $held ->
    [one] { $held } chanson
   *[other] { $held } chansons
} déjà détenu
left-out = { $held }, { $untitled ->
    [one] { $untitled } chanson
   *[other] { $untitled } chansons
} laissés en dehors sans titre
walked = A débarqué { $landed }- A disparu { $gone }- Détenu { $unkept }
premium = Formats premium disponibles
without-premium = signé, mais ce compte n’a pas de formats Premium
not-signed-in = non signé en
tool-missing = yt-dlp n’a pas pu être exécuté; c’est une installation séparée
check-song-gone = la chanson de ce chèque demande a quitté YouTube, donc rien ne pouvait être dit
could-not-check = YouTube ne pouvait pas être demandé tout de suite; essayez de nouveau dans un moment
window-open = une fenêtre est ouverte sur la connexion de Google: connectez-vous là, et il se ferme une fois YouTube Music est atteint
signed-in = signé à YouTube Music; youtube-music-cookies nommait les propres cookies de la fenêtre maintenant
signed-out = signé de YouTube Music: les cookies et le profil de la fenêtre ont disparu, et youtube-music-cookies avec eux
window-forgotten = les cookies et le profil de la fenêtre de connexion sont partis; youtube-music-cookies nomme autre chose et est laissé comme il était
