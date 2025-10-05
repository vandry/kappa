module.exports = (env) => {
  return {
    entry: './main.js',
    resolve: {
      alias: {
        codegen: env.out_dir,
      },
    },
    output: {
      filename: '../bundle.js',
      path: env.out_dir,
    },
    optimization: {
      minimize: false,
    },
  };
};
