local_file=${EXE:-moldy}

echo "Build..."
docker build -t mold .

echo "Start..."
container=$(docker run --rm --detach --tty mold)

echo "Copy..."
docker cp $container:/home/rust/mold $local_file

echo "Kill..."
docker kill $container

echo "Done."
ls $local_file
