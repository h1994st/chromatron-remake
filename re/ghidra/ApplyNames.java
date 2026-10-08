// Applies our function/label names from re/notes/chromatron<N>/*.names.tsv before export.
//@category Chromatron
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.SourceType;
import java.nio.file.*;
import java.util.stream.Stream;

public class ApplyNames extends GhidraScript {
    @Override
    protected void run() throws Exception {
        Path dir = Paths.get(getScriptArgs()[0]);
        if (!Files.isDirectory(dir)) {
            println("no names directory: " + dir);
            return;
        }
        try (Stream<Path> files = Files.list(dir)) {
            for (Path file : (Iterable<Path>) files.filter(p -> p.toString().endsWith(".names.tsv")).sorted()::iterator) {
                int lineNo = 0;
                for (String line : Files.readAllLines(file)) {
                    lineNo++;
                    if (line.isBlank() || line.startsWith("#")) continue;
                    String[] f = line.split("\t");
                    if (f.length != 3) throw new IllegalArgumentException(file + ": bad line: " + line);
                    Address at = toAddr(f[0]);
                    if (f[2].equals("fn")) {
                        Function fn = getFunctionAt(at);
                        if (fn == null) {
                            if (createFunction(at, f[1]) == null) throw new IllegalStateException(file + ":" + lineNo + ": createFunction failed: " + line);
                        } else fn.setName(f[1], SourceType.USER_DEFINED);
                    } else if (f[2].equals("data")) {
                        if (createLabel(at, f[1], true, SourceType.USER_DEFINED) == null) throw new IllegalStateException(file + ":" + lineNo + ": createLabel failed: " + line);
                    } else {
                        throw new IllegalArgumentException(file + ": kind must be fn or data: " + line);
                    }
                }
            }
        }
    }
}
