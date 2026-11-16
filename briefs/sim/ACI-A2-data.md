# Agent A2: data, network and systems (read ACI-AGENTS.md first; work folder $SB/work2)

1. Create a database file shop.db with a table items (id INTEGER PRIMARY KEY, name TEXT, price REAL)
   and insert three rows with prices 10.5, 20 and 30.25. Report the average price.
2. Insert 250 more rows (one statement or a few), then read rows 201 to 250 of `SELECT * FROM items`
   using the tool's paging. Report the count you got and whether more remain.
3. Insert a row whose id already exists: report the error code.
4. Query a table that does not exist: report the error code.
5. Make a filesystem image disk.img of 256 blocks; create the folder /docs inside it.
6. Put a host file (any text) into the image as /docs/a.txt; list /docs and report name, kind and size.
7. Read /docs/a.txt back from the image: the text must match the host file.
8. Put a 20000-byte file into the image, read it back as text (report truncated and bytes), then read it
   to a host file and check it is byte-identical.
9. Check the image's consistency: report clean and the problem count.
10. Try to make a filesystem of 4 blocks: report the error code.
11. Hash a host file with SHA-256 using the crypto tool and confirm the hash with python's hashlib.
12. Make a signing key, sign a file, verify the signature: report valid.
13. Change one byte of the file and verify again: report valid and ok.
14. Make a 32-byte key file (python os.urandom is fine), encrypt a file with it, decrypt it: the
    plaintext must match. Then decrypt with a different key: report the error code.
15. Compute an HMAC-SHA-256 of a file with the hex key `4a656665`: report the mac.
16. Serve a folder holding a file hello.txt with the HTTP tool on 127.0.0.1 port 0 in the background.
    Read the bound address from its first line.
17. Fetch hello.txt from that server: report status, the Content-Length header and the body.
18. Fetch a path that does not exist: report status and ok. Then stop the server by PID.
19. Fetch an https:// URL: report the error code and hint.
20. Parse samples/req.http as an HTTP message: report kind, method and path.
21. From samples/shop.html list every link element with paging limit 2: report the first page's
    hrefs, total and next_offset.
22. Give the browser engine an invalid CSS selector: report the error code.
23. Render samples/shop.html to a PNG file: report width, height, and confirm the PNG signature.
24. Get the DOM tree of shop.html: report the depth and name of the `li` nodes.
25. Make a demo blockchain file, then verify it: report height and hash, and that both commands agree.
26. Make a key with the encrypted-transport tool: report which fields it returned.
27. Run the benchmark's fastest suite only: report one result with its unit.
28. Run the assembly kernel for 100000 steps: report how it stopped and the last line of stdout.
29. Boot the OS with samples/titan.txt as its script: confirm `hi from titan` is in its output.
30. Ask the container tool for its container list: report what it says on this Windows machine.
