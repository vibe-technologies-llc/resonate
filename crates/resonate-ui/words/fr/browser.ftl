add-a-folder = Ajoutez un dossier musical à partir de Paramètres pour en scanner un.
add-songs = Démarrez votre bibliothèque
add-songs-more = Choisissez le dossier de musique primaire : les chansons que vous faites glisser sur la fenêtre y sont copiées, et ce qui est déjà là est scanné.
choose-primary-hint = Ouvrez le dialogue du dossier et faites que le dossier choisi le dossier de musique primaire
drop-songs-more = Faites glisser les chansons sur la fenêtre, ou mettez-les dans le dossier de musique primaire, pour remplir la bibliothèque.
library-unreadable = La bibliothèque n’a pas pu être lue
library-unreadable-more = Le journal dit ce qui a mal tourné en lisant le catalogue.
nothing-held-matches = Rien dans votre bibliothèque ne correspond à cette recherche.
several = Artistes divers
forget-the-match = Pas ce disque
other-pressings = Autres pressages
other-pressings-hint = Lister chaque pression de ce document MusicBrainz détient, et prendre celui que ces fichiers sont
asking-for-pressings = Demander MusicBrainz les pressages...
no-pressings = MusicBrainz n’a pas donné le nom d’une pression
find-the-record = Trouvez l’enregistrement
find-the-record-hint = Demandez MusicBrainz pour les versions nommées comme cet album, et prenez celui que ces fichiers sont
asking-for-releases = Demander MusicBrainz des libérations nommées comme ça...
no-releases = MusicBrainz n’a donné aucun nom à cette version
take-the-pressing-hint = Prenez cette pression pour l’album en lieu et place de celui utilisé
pressing-in-use-hint = La pression sur l’album est jumelée à maintenant
forget-the-match-hint = La recherche a pris la mauvaise version : oublie-la, et ne la reprend plus jamais pour cet album
forget-the-match-armed = Appuyez encore pour oublier le match
forget-the-match-armed-hint = Retire la version, ses lignes et ce qu’elle lie, et la prochaine recherche demande à nouveau sans elle
order-hint = Choisissez ce que cette liste est mise en ordre par
play-all-hint = Jouez chaque morceau listé ici dans l’ordre, en lieu et place de la file d’attente
shuffle-all-hint = Jouer chaque morceau listé ici, shuffled
open-album-hint = Voir les titres de cet album
open-artist-hint = Voir tous les titres de cet artiste
instead-hint = Rechercher l’orthographe du catalogue tient à la place
sung-hint = Rechercher les paroles de ces mots, dans l’ordre où ils ont été tapés
want-hint = Marquer cette piste souhaitée, afin qu’un fournisseur puisse être demandé
unwant-hint = Arrête de vouloir cette piste.
dismiss-missing-hint = Enlevez cette piste de la liste des disparus, et arrêtez de la vouloir si elle était voulue
want-found-hint = Téléchargez cette chanson : sa sortie est ajoutée au catalogue et les fournisseurs sont invités à la faire. Faites un clic droit pour choisir la sortie
fetch-found-hint = Télécharger cette chanson: sa sortie est ajoutée au catalogue et les fournisseurs sont invités à la faire maintenant, la barre latérale suivant comment elle va
opening = Ouverture...
open-artist-found-hint = Ouvrez cet artiste pour voir leurs sorties et télécharger les chansons que vous voulez
not-in-the-library = pas dans votre bibliothèque
open-album-not-held-hint = Ouvrez cet album pour voir ses chansons et télécharger celles que vous voulez
get-album-rest-hint = Demandez aux fournisseurs de chaque morceau manquant sur cet album
fetch-found-again-hint = Demandez encore aux fournisseurs cette chanson
place-on = Placez-vous sur une libération...
look-it-up = Enrichir cette chanson
read-the-rest = Lire le reste
read-the-rest-hint = Demandez MusicBrainz pour les sorties passées les mille premiers que cet artiste est crédité sur
nothing-places-it = Rien n’a identifié cette piste, donc elle n’est pas encore disponible
unheld-hint = Voyez ce que cet artiste a publié que la bibliothèque ne tient pas
record-hint = À propos de ce dossier
genres-hint = Genres
favour-hint = Gardez ceci parmi vos favoris
unfavour-hint = Enlevez ça de vos favoris.
add-songs-label = Ajouter des chansons
no-albums-match = Aucun album ne correspond
no-albums-yet = Pas encore d’albums
albums = Albums
set-primary-folder = Définir le dossier primaire...
no-artists-match = Aucun artiste ne correspond
no-artists-yet = Pas encore d’artistes
artists = Artistes
no-tracks-yet = Pas encore de pistes
no-tracks-match = Aucune trace ne correspond.
list = Liste
grid = Grille
all-tracks = Toutes les pistes
done = Terminé
save-this-search = Enregistrer cette recherche
album = Album
artist = Artiste
tracks = Pistes
in-use = En usage
play = Lire
shuffle = Lecture aléatoire
artists-list-hint = Dessinez les artistes comme une liste, un petit portrait à côté de chaque nom
artists-grid-hint = Dessiner les artistes comme une grille de grands portraits, la façon dont les albums sont
find-the-releases-it-is-on = trouver les versions sur lesquelles il est
released = Libéré
format = Format
label = Étiquette
catalogue-number = Numéro de catalogue
barcode = Code barre
country = Pays
kind = Genre
about-this-record = À propos de ce dossier
genres = Genres
on = Activé
all-lossless = tous sans perte
albums-counted = { $count ->
    [one] { $count } album
   *[other] { $count } albums
}
artists-counted = { $count ->
    [one] { $count } artiste
   *[other] { $count } artistes
}
tracks-counted = { $count ->
    [one] { $count } piste
   *[other] { $count } voies
}
plays-counted = { $count ->
    [one] { $count } jouer
   *[other] { $count } pièces
}
did-you-mean = Vous vouliez dire { $instead } ?
sung-in = Chanté dans { $count ->
    [one] { $count } piste
   *[other] { $count } voies
}
songs-not-held = Pas dans votre bibliothèque · { $count ->
    [one] { $count } chanson
   *[other] { $count } chansons
} · appuyez sur un pour le télécharger
add-songs-to = Ajouter des chansons à { $playlist }
choose-tracks-to-add-to = Choisissez des pistes à ajouter à { $playlist }
unnamed-album = album { $id }
releases-not-held = { $count ->
    [one] { $count } libération
   *[other] { $count } rejets
} non détenu
releases-not-held-more-unread = { $count ->
    [one] { $count } libération
   *[other] { $count } rejets
} non retenu, { $unread } plus non lu
pressings = Presses · { $count }
and-more = et { $count } plus
undated = non daté
pressing-tracks = { $count } voies
releases-not-in-your-library = Pas dans votre bibliothèque · { $count ->
    [one] { $count } libération
   *[other] { $count } rejets
}
disc-named = Disque { $disc } · { $named }
disc = Disque { $disc }
discs = { $count } disques
on-services = Sur{"\u00A0"}
open-on = Ouvrir le { $service }
unnamed-artist = artiste { $id }
lossless = { $count } sans perte
shown = { $named } · { $drawn } montré
way-back-hint = De retour à l’endroit où cela s’ouvrait — { $keys }
