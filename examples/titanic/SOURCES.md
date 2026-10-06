# Sources

Two official inquiries into the loss of the Titanic, which disagree on two
facts: when the iceberg was sighted and how many people were saved. Each
directory is one independence group.

| File | Source | Page |
| --- | --- | --- |
| `sources/us-senate/report.md` | *"Titanic" Disaster: Report of the Committee on Commerce, United States Senate* (Senate Report No. 806, 62nd Congress, 1912) | [The collision](https://www.titanicinquiry.org/USInq/USReport/AmInqRep05.php), [Capacity of lifeboats not utilized](https://www.titanicinquiry.org/USInq/USReport/AmInqRep07.php) |
| `sources/british-inquiry/report.md` | *Report on the Loss of the "Titanic" (s.s.)*, British Wreck Commissioner's Inquiry (Cd. 6352, 1912) | [The collision](https://www.titanicinquiry.org/BOTInq/BOTReport/botRepCollision.php), [The rescue by the Carpathia](https://www.titanicinquiry.org/BOTInq/BOTReport/botRepRescue.php) |

## The disagreements

- **Time.** The US report: "At 11.46 p.m. ship's time … the lookout signaled
  the bridge". The British report: "at a little before 11.40, one of the
  look-outs in the crow's nest struck three blows on the gong".
- **Survivors.** The US report: "706 were saved". The British report: the
  Carpathia "took on board 712 persons, one of them died shortly afterwards".

## Licence

- The US report is a work of the United States federal government, so it is
  in the public domain (17 U.S.C. § 105).
- The British report is a Crown publication from 1912. Crown copyright in a
  published work lasts 50 years from publication, so it expired at the end
  of 1962.
- The pages above are faithful transcriptions, which add no copyright of
  their own.

## Editing

The excerpts are verbatim, with these changes only:

- The headings are ours.
- Each file keeps one sentence from each page, the one that disagrees with
  the other report, and drops the rest. Short excerpts keep the script
  request inside a small local model's context window (Ollama's default is
  4096 tokens).
- The witness reference "(Hichens, 969)" was removed from the British
  report's collision sentence.
- In the British rescue sentence, "he" is Arthur Rostron, the Carpathia's
  captain.
