back-hint = De retour à l’endroit où cela a été ouvert de
play-hint = Jouez tout ici, dans l’ordre
next-hint = Jouer ceci suivant
queue-hint = Ajouter ceci à la fin de la file d’attente
open-hint = Voir ce que le serveur détient sous cette
play-this-hint = Jouer ceci, et ce qui suit ici
not-played-here-hint = Les chansons de ce service ne sont pas jouées d’ici.
keep-hint = Gardez cette chanson dans le dossier musical
keep-all-hint = Gardez chaque chanson ici dans le dossier musical, comme une liste de lecture
ask-again-hint = Demandez à nouveau au serveur
search-placeholder = Rechercher le serveur, puis appuyez sur Entrée
nothing-here = Rien ici.
nothing-listed = Rechercher pour commencer
favourites = Favoris
media-servers = Serveurs multimédias
media-server = Serveur multimédia
folder-on-the-server = Dossier sur le serveur
no-server-answered = Aucun serveur multimédia n’a répondu
no-server-answered-more = Les serveurs multimédias de ce réseau sont recherchés chaque fois que cette page s’ouvre. Vérifiez que le serveur est en cours d’exécution et sur le même réseau que cet ordinateur.
search-again = Recherche à nouveau
search-again-hint = Cherchez à nouveau des serveurs multimédias sur le réseau
search-first = Ce service ne liste rien jusqu’à ce qu’il soit demandé. Recherche ci-dessus pour un artiste, un album ou une chanson.
choose-hint = Parcourir ce service
choose-among-hint = Choisissez le service à parcourir
nothing-found = Rien trouvé
asking = Demande au serveur...
asking-more = Ce qu’il contient ici apparaît dès qu’il répond.
nothing-found-more = Le service n’a rien pour cette recherche. Essayez d’autres mots.
nothing-here-more = Le service n’a rien à voir là-dedans.
try-again = Essaie encore
play = Lire
keep = Gardez
favourite-unchanged = Ne pouvait pas changer ce favori — { $failure }
unread = Je ne pouvais pas lire ça... { $failure }
artists = { $count ->
    [one] { $count } artiste
   *[other] { $count } artistes
}
albums = { $count ->
    [one] { $count } album
   *[other] { $count } albums
}
servers = { $count ->
    [one] { $count } serveur
   *[other] { $count } serveurs
}
folders = { $count ->
    [one] { $count } dossier
   *[other] { $count } dossiers
}
mixes = { $count ->
    [one] { $count } mélange
   *[other] { $count } mélanges
}
songs = { $count ->
    [one] { $count } chanson
   *[other] { $count } chansons
}
queued-and-playing = { $count ->
    [one] { $count } chanson en attente et en train de jouer
   *[other] { $count } chansons en attente et en train de jouer
}
to-play-next = { $count ->
    [one] { $count } chanson pour jouer la suivante
   *[other] { $count } chansons à jouer
}
added-to-the-queue = { $count ->
    [one] { $count } chanson ajoutée à la file d’attente
   *[other] { $count } chansons ajoutées à la file d’attente
}
