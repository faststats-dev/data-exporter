import { S3Client } from "bun";

const client = new S3Client({
  accessKeyId: process.env.S3_ACCESS_KEY_ID,
  secretAccessKey: process.env.S3_SECRET_ACCESS_KEY,
  bucket: process.env.S3_BUCKET,
  region: process.env.S3_REGION,
  endpoint: process.env.S3_ENDPOINT,
});

async function purgeBucket() {
  console.log(`Starting purge for bucket: ${process.env.S3_BUCKET}...`);
  let totalDeleted = 0;
  let isTruncated = true;
  let startAfter: string | undefined = undefined;

  try {
    while (isTruncated) {
      // List objects in the bucket
      const response = await client.list({
        maxKeys: 1000,
        startAfter,
      });

      const objects = response.contents || [];
      
      if (objects.length === 0) {
        break;
      }

      // Perform deletions in parallel for the current batch
      await Promise.all(
        objects.map((obj) => {
          console.log(`Deleting: ${obj.key}`);
          return client.delete(obj.key);
        })
      );

      totalDeleted += objects.length;
      isTruncated = response.isTruncated;
      if (isTruncated && objects.length > 0) {
        startAfter = objects[objects.length - 1].key;
      }
    }

    console.log(`\nSuccessfully purged ${totalDeleted} objects.`);
  } catch (error) {
    console.error("Error purging bucket:", error);
    process.exit(1);
  }
}

purgeBucket();