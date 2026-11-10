genuine = Consistent with lossless
suspect = Suspect: it may be a high-bitrate lossy encode
fake = Fake: it is not the lossless master it claims to be
lossy = Lossy by its codec
not-judged = Not judged
wall-of-a-lossy-encode = The spectrum ends in a wall at { $kilohertz } kHz, { $decibels } dB deep, where a lossy encode near { $kbps } kbps cuts
wall-of-a-high-bitrate-encode = The spectrum ends in a wall at { $kilohertz } kHz, { $decibels } dB deep, where a high-bitrate lossy encode cuts, or an unusually low anti-alias filter
wall-of-an-anti-alias-filter = The spectrum ends at { $kilohertz } kHz, { $decibels } dB deep, where an anti-alias filter would stand
rolloff = The spectrum fades out by { $kilohertz } kHz with no wall to judge by
upsampled = Nothing stands above { $nyquist } kHz, where { $kilohertz } kHz audio ends, so it was upsampled from it
padded = Only { $effective } of its { $declared } bits carry audio; the rest are padding
too-little-heard = Too little of it is loud enough to judge
from-dsd = A DSD stream is not judged by the spectrum its decimation leaves
mono-as-stereo = Both channels carry the same samples
clipped = { $samples ->
    [one] { $samples } sample sits
   *[other] { $samples } samples sit
} at full scale in { $runs ->
    [one] { $runs } run
   *[other] { $runs } runs
}
dc-offset = The signal sits off centre by { $decibels } dBFS
quantiser-steps = Its transform on { $coding }'s own frames lands on { $coding }'s quantiser steps ({ $aligned }% aligned), so it was decoded from { $coding }
quantiser-steps-resampled = Brought back to { $kilohertz } kHz, its transform on { $coding }'s own frames lands on { $coding }'s quantiser steps ({ $aligned }% aligned), so it was decoded from { $coding } at that rate and resampled
intensity-stereo = On { $coding }'s own frames its two channels move as one in { $share }% of { $coding }'s bands, exactly as { $coding }'s intensity stereo leaves them and far more than off those frames, so it was decoded from { $coding }
intensity-stereo-resampled = Brought back to { $kilohertz } kHz, on { $coding }'s own frames its two channels move as one in { $share }% of { $coding }'s bands, exactly as { $coding }'s intensity stereo leaves them and far more than off those frames, so it was decoded from { $coding } at that rate and resampled
