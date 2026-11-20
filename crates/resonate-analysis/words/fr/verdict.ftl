genuine = Conformément au principe de l ’ absence de pertes
suspect = Suspect : il peut s’agir d’un code de perte à haut débit
fake = Faux: ce n’est pas le maître sans perte qu’il prétend être
lossy = Perte par son codec
not-judged = Non jugé
wall-of-a-lossy-encode = Le spectre se termine dans un mur à { $kilohertz } kHz, { $decibels } dB de profondeur, où un code perdu près de { $kbps } kbps coupe
wall-of-a-high-bitrate-encode = Le spectre se termine par une paroi à { $kilohertz } kHz, { $decibels } dB de profondeur, où un code de perte à haut débit coupe, ou un filtre anti-alias exceptionnellement bas
wall-of-an-anti-alias-filter = Le spectre se termine à { $kilohertz } kHz, { $decibels } dB de profondeur, où un filtre anti-alias resterait
rolloff = Le spectre s’efface par { $kilohertz } kHz sans mur pour juger par
upsampled = Rien n’est au-dessus de { $nyquist } kHz, où { $kilohertz } kHz son se termine, donc il a été relevé de lui
padded = Seulement { $effective } de ses bits { $declared } transportent de l’audio; les autres sont rembourrés
too-little-heard = Trop peu, c’est assez fort pour juger.
from-dsd = Un flux DSD n’est pas jugé par le spectre de ses feuilles de décimation
mono-as-stereo = Les deux canaux transportent les mêmes échantillons
clipped = { $samples ->
    [one] { $samples } l’échantillon est assis
   *[other] { $samples } échantillons assis
} à l’échelle { $runs ->
    [one] { $runs } lancer
   *[other] { $runs } pistes
}
dc-offset = Le signal s’arrête au centre par { $decibels } dBFS
quantiser-steps = Sa transformation sur les propres cadres de { $coding } atterrit sur les pas quantificateurs de { $coding } ({ $aligned }% aligné), donc il a été décodé de { $coding }
quantiser-steps-resampled = Rajouté à { $kilohertz } kHz, sa transformation sur les propres cadres { $coding } atterrit sur les étapes de quantification { $coding } ({ $aligned }% aligné), de sorte qu’il a été décodé de { $coding } à ce taux et rééchantillonné
intensity-stereo = Sur les propres cadres de { $coding } ses deux canaux se déplacent comme un dans { $share }% des bandes de { $coding }, exactement comme l’intensité stéréo de { $coding } les laisse et bien plus que hors de ces cadres, de sorte qu’il a été décodé de { $coding }
intensity-stereo-resampled = Rajouté à { $kilohertz } kHz, sur les propres cadres de { $coding } ses deux canaux se déplacent comme un dans { $share }% des bandes de { $coding }, exactement comme { $coding } l’intensité stéréo les laisse et bien plus que ces cadres, il a donc été décodé de { $coding } à cette vitesse et rééchantillonné
