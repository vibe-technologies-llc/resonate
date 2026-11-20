none-lossy = rien en { $name } ne pouvait être entendu dans une qualité supérieure
held-copy = joue la copie { $codec } tenue à côté
wanted = veut, donc `resonate poll` récupère une copie sans perte
no-release = aucune copie de qualité supérieure tenue et aucune version à vouloir
already-waiting = déjà en attente d’une copie sans perte
offered = trouvé sans perte sur { $provider }, donc il est téléchargé à partir de là
can-be-replaced = { $songs ->
    [one] { $replaceable } de { $songs } chanson peut jouer une version de meilleure qualité maintenant
   *[other] { $replaceable } de { $songs } chansons peuvent jouer une version de meilleure qualité maintenant
}
in-playlist = { $name }: { $parts }
replaced = { $songs ->
    [one] { $songs } chanson joue maintenant la copie de qualité supérieure tenue à côté de elle
   *[other] { $songs } chansons jouent maintenant les copies de qualité supérieure tenues à côté d’eux
}
now-wanted = { $songs ->
    [one] il voulait { $songs } chanson, qui `resonate poll` récupère dans une copie sans perte
   *[other] veut { $songs } chansons, qui `resonate poll` récupère en copies sans perte
}
now-kept = { $songs ->
    [one] filed { $songs } chanson à télécharger du service le tenant sans perte, que la fenêtre télécharge une fois qu’elle s’ouvre
   *[other] filed { $songs } chansons à télécharger des services les tenant sans perte, que la fenêtre télécharge une fois qu’elle s’ouvre
}
unfiled = { $songs ->
    [one] { $songs } chanson trouvée sans perte sur un service a besoin d’un dossier de musique à télécharger dans
   *[other] { $songs } chansons trouvées sans perte sur un service ont besoin d’un dossier de musique à télécharger dans
}
waiting = { $songs } déjà en attente d’une copie sans perte
left = gauche { $songs } sans copie de qualité supérieure conservée et sans sortie à vouloir
nothing-changed = rien en { $name } ne pouvait être entendu dans une qualité supérieure
