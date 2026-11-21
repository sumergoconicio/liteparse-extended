// Run from AskClaire-MONO/orchestrator with -mod=readonly. Writes JSON to stdout.
// The inspected implementation is the reference, not a second invented scorer.
package main

import (
	"encoding/json"
	"os"
	"unicode"

	"orchestrator/framework/utilities/jsoninspect"
)

func main() {
	type sample struct {
		Text string `json:"text"`
		Score float64 `json:"score"`
	}
	texts := []string{
		"", "short", "alpha beta gamma", "Alpha beta gamma.",
		"We can inspect the original source and verify the evidence in this answer.",
		"This is another native note about the source and its evidence.",
		`if (x == y) { return a; }`, "the and of", "  This is a sentence.  ",
		"Étude précise demeure.", "École régionale ferme.",
		"Ⅳ alpha beta.", "中文 文本 测试。", "Alpha\u0085beta\u2003gamma.",
		"“the” unquoted words.",
	}
	result := struct {
		UnicodeVersion string `json:"unicode_version"`
		Samples []sample `json:"samples"`
	}{UnicodeVersion: unicode.Version}
	for _, text := range texts {
		result.Samples = append(result.Samples, sample{text, jsoninspect.CalculateProseDensity(text)})
	}
	encoder := json.NewEncoder(os.Stdout)
	encoder.SetIndent("", "  ")
	if err := encoder.Encode(result); err != nil { panic(err) }
}
