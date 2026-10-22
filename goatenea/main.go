// Atenea Go shell: all UI lives here (Fyne). The Rust engine is a
// sidecar: `acore <summary|tables|wires|sim> <file>` prints JSON.
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/app"
	"fyne.io/fyne/v2/container"
	"fyne.io/fyne/v2/widget"
)

var acoreBin = "acore"

func init() {
	if p, err := exec.LookPath("acore"); err == nil {
		acoreBin = p
	} else if home, err := os.UserHomeDir(); err == nil {
		cand := filepath.Join(home, ".local", "bin", "acore")
		if _, err := os.Stat(cand); err == nil {
			acoreBin = cand
		}
	}
}

func runAcore(cmd, file string, args ...string) ([]byte, error) {
	argv := append([]string{cmd, file}, args...)
	out, err := exec.Command(acoreBin, argv...).Output()
	if err != nil {
		if ee, ok := err.(*exec.ExitError); ok {
			return nil, fmt.Errorf("acore: %s", string(ee.Stderr))
		}
		return nil, err
	}
	return out, nil
}

type summaryDoc struct {
	File    string            `json:"file"`
	Kind    string            `json:"kind"`
	Summary map[string]string `json:"summary"`
	Counts  map[string]int    `json:"counts"`
}

type partRow struct {
	Ref   string   `json:"ref"`
	Kind  string   `json:"kind"`
	Value string   `json:"value"`
	Rest  []string `json:"rest"`
}

type netRow struct {
	ID   string     `json:"id"`
	Pins [][]string `json:"pins"`
}

type tablesDoc struct {
	File  string    `json:"file"`
	Parts []partRow `json:"parts"`
	Nets  []netRow  `json:"nets"`
}

type state struct {
	w      fyne.Window
	status *widget.Label
	tabs   *container.AppTabs
}

func (s *state) setStatus(msg string) {
	s.status.SetText(msg)
}

func (s *state) openFile(path string) {
	out, err := runAcore("summary", path)
	if err != nil {
		s.setStatus("error: " + err.Error())
		return
	}
	var sum summaryDoc
	if err := json.Unmarshal(out, &sum); err != nil {
		s.setStatus("error: bad acore json")
		return
	}
	s.setStatus(fmt.Sprintf("%s · %s", filepath.Base(path), sum.Kind))
	s.showTables(path)
}

func (s *state) showTables(path string) {
	out, err := runAcore("tables", path)
	if err != nil {
		// DSN/LYT/pdsprj have no part tables: show summary instead.
		sout, serr := runAcore("summary", path)
		if serr != nil {
			s.setStatus("error: " + err.Error())
			return
		}
		var sum summaryDoc
		_ = json.Unmarshal(sout, &sum)
		rows := [][]string{{"campo", "valor"}}
		for k, v := range sum.Summary {
			if k == "_" {
				continue
			}
			rows = append(rows, []string{k, v})
		}
		for k, v := range sum.Counts {
			rows = append(rows, []string{k, fmt.Sprint(v)})
		}
		t := widget.NewTable(
			func() (int, int) { return len(rows), 2 },
			func() fyne.CanvasObject { return widget.NewLabel("") },
			func(id widget.TableCellID, o fyne.CanvasObject) {
				o.(*widget.Label).SetText(rows[id.Row][id.Col])
			},
		)
		for _, tab := range s.tabs.Items {
			if tab.Text == "Tables" {
				tab.Content = container.NewScroll(t)
				s.tabs.Refresh()
				s.tabs.Select(tab)
				return
			}
		}
		return
	}
	var doc tablesDoc
	if err := json.Unmarshal(out, &doc); err != nil {
		s.setStatus("error: bad acore json")
		return
	}
	rows := [][]string{{"Ref", "Tipo", "Valor"}}
	for _, p := range doc.Parts {
		rows = append(rows, []string{p.Ref, p.Kind, p.Value})
	}
	t := widget.NewTable(
		func() (int, int) { return len(rows), 3 },
		func() fyne.CanvasObject { return widget.NewLabel("") },
		func(id widget.TableCellID, o fyne.CanvasObject) {
			o.(*widget.Label).SetText(rows[id.Row][id.Col])
		},
	)
	for _, tab := range s.tabs.Items {
		if tab.Text == "Tables" {
			tab.Content = container.NewScroll(t)
			s.tabs.Refresh()
			s.tabs.Select(tab)
			return
		}
	}
}

func homeContent(s *state) fyne.CanvasObject {
	title := widget.NewLabelWithStyle("ATENEA — clon nativo interoperable",
		fyne.TextAlignLeading, fyne.TextStyle{Bold: true})
	pathEntry := widget.NewEntry()
	pathEntry.SetPlaceHolder("/ruta/al/archivo.pdsprj")
	open := widget.NewButton("Abrir", func() {
		if pathEntry.Text != "" {
			s.openFile(pathEntry.Text)
		}
	})
	cards := container.NewVBox(
		title,
		widget.NewLabel("Abrí un .pdsprj, .DSN, .MDF, .IDX para empezar."),
		pathEntry,
		open,
		widget.NewLabel(".pdsprj  .MDF  .SDF  .DSN  .LYT  .IDX"),
	)
	return container.NewScroll(cards)
}

func main() {
	a := app.NewWithID("atenea.go")
	w := a.NewWindow("Atenea")
	w.Resize(fyne.NewSize(1280, 800))
	st := &state{w: w, status: widget.NewLabel("listo")}
	st.tabs = container.NewAppTabs(
		container.NewTabItem("Tables", widget.NewLabel("sin archivo")),
		container.NewTabItem("Graph", widget.NewLabel("vista topológica (próximo slice)")),
		container.NewTabItem("Esquema", widget.NewLabel("hoja ISIS (próximo slice)")),
		container.NewTabItem("Simulate", widget.NewLabel("simulación batch (próximo slice)")),
	)
	openItem := fyne.NewMenuItem("Abrir…", func() {
		st.tabs.SelectIndex(0)
	})
	fileMenu := fyne.NewMenu("File", openItem, fyne.NewMenuItem("Salir", func() { a.Quit() }))
	helpMenu := fyne.NewMenu("Help", fyne.NewMenuItem("Acerca de", func() {
		st.setStatus("Atenea GPL-3.0-or-later · RE interoperable")
	}))
	w.SetMainMenu(fyne.NewMainMenu(fileMenu, helpMenu))
	home := homeContent(st)
	split := container.NewHSplit(home, st.tabs)
	split.Offset = 0.28
	w.SetContent(container.NewBorder(nil, st.status, nil, nil, split))
	if len(os.Args) > 1 {
		st.openFile(os.Args[1])
	}
	w.ShowAndRun()
}
